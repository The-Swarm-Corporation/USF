use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Write, Seek, SeekFrom};
use std::path::Path;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use image::{ImageFormat};
use zstd;
use bincode;
use xxhash_rust::xxh3::xxh3_64;
use std::io::Cursor;

const MAGIC_BYTES: &[u8; 4] = b"USF1";
const VERSION: u8 = 1;
const BLOCK_SIZE: usize = 1024 * 64; // 64KB blocks
const MIN_COMPRESS_SIZE: usize = 1024; // Minimum size to attempt compression

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum DataType {
    Text,
    Binary,
    Image,
    Json,
    Structured,
}

#[derive(Serialize, Deserialize, Debug)]
struct BlockHeader {
    data_type: DataType,
    original_size: u64,
    compressed_size: u64,
    compression_method: CompressionMethod,
    checksum: u64,
    timestamp: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
enum CompressionMethod {
    None,
    Zstd,
    DeltaEncoding,
}

#[derive(Serialize, Deserialize, Debug)]
struct MetaData {
    created: DateTime<Utc>,
    modified: DateTime<Utc>,
    total_blocks: u64,
    index: HashMap<String, BlockLocation>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct BlockLocation {
    offset: u64,
    header_size: u32,
    data_size: u64,
}

pub struct UniversalStorage {
    file: File,
    metadata: MetaData,
}

impl UniversalStorage {
    pub fn create<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut file = File::create(path)?;
        file.write_all(MAGIC_BYTES)?;
        file.write_all(&[VERSION])?;

        let metadata = MetaData {
            created: Utc::now(),
            modified: Utc::now(),
            total_blocks: 0,
            index: HashMap::new(),
        };

        let metadata_bytes = bincode::serialize(&metadata)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        
        let metadata_size = metadata_bytes.len() as u64;
        file.write_all(&metadata_size.to_le_bytes())?;
        file.write_all(&metadata_bytes)?;

        Ok(Self { file, metadata })
    }

    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let mut magic = [0u8; 4];
        file.read_exact(&mut magic)?;
        
        if &magic != MAGIC_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid file format"));
        }

        let mut version = [0u8];
        file.read_exact(&mut version)?;
        if version[0] != VERSION {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Unsupported version"));
        }

        let mut size_bytes = [0u8; 8];
        file.read_exact(&mut size_bytes)?;
        let metadata_size = u64::from_le_bytes(size_bytes);

        let mut metadata_bytes = vec![0u8; metadata_size as usize];
        file.read_exact(&mut metadata_bytes)?;

        let metadata: MetaData = bincode::deserialize(&metadata_bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        Ok(Self { file, metadata })
    }

    pub fn store(&mut self, key: &str, data: &[u8], data_type: DataType) -> io::Result<()> {
        let blocks = self.prepare_blocks(data, data_type)?;
        let mut locations = Vec::new();

        for block in blocks {
            let location = self.write_block(&block)?;
            locations.push(location);
        }

        // Update index with first block location
        if let Some(first_location) = locations.first() {
            self.metadata.index.insert(key.to_string(), first_location.clone());
            self.metadata.total_blocks += locations.len() as u64;
            self.metadata.modified = Utc::now();
            self.update_metadata()?;
        }

        Ok(())
    }

    pub fn retrieve(&mut self, key: &str) -> io::Result<Vec<u8>> {
        let location = self.metadata.index.get(key)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Key not found"))?;
    
        let mut result = Vec::new();
        let mut current_location = Some(location.clone());
    
        while let Some(loc) = current_location {
            let block = self.read_block(&loc)?;
            
            // Verify checksum
            let checksum = xxh3_64(&block.data);
            if checksum != block.header.checksum {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Data corruption detected"));
            }
    
            result.extend_from_slice(&block.data);
            // current_location = block.next_location;  // This should work now that BlockLocation implements Clone
            current_location = block.next_location;
        }
    
        Ok(result)
    }

    fn prepare_blocks(&self, data: &[u8], data_type: DataType) -> io::Result<Vec<Block>> {
        let mut blocks = Vec::new();
        let mut offset = 0;

        while offset < data.len() {
            let chunk_size = std::cmp::min(BLOCK_SIZE, data.len() - offset);
            let chunk = &data[offset..offset + chunk_size];

            let (compressed_data, method) = if chunk.len() >= MIN_COMPRESS_SIZE {
                match self.compress_data(chunk, &data_type) {
                    Ok((compressed, method)) => (compressed, method),
                    Err(_) => (chunk.to_vec(), CompressionMethod::None),
                }
            } else {
                (chunk.to_vec(), CompressionMethod::None)
            };

            let checksum = xxh3_64(&compressed_data);

            let header = BlockHeader {
                data_type: data_type.clone(),
                original_size: chunk.len() as u64,
                compressed_size: compressed_data.len() as u64,
                compression_method: method,
                checksum,
                timestamp: Utc::now(),
            };

            blocks.push(Block {
                header,
                data: compressed_data,
                next_location: None,
            });

            offset += chunk_size;
        }

        // Link blocks together
        for i in 0..blocks.len() - 1 {
            blocks[i].next_location = Some(BlockLocation {
                offset: 0, // Will be set during writing
                header_size: 0,
                data_size: blocks[i + 1].data.len() as u64,
            });
        }

        Ok(blocks)
    }

    fn compress_data(&self, data: &[u8], data_type: &DataType) -> io::Result<(Vec<u8>, CompressionMethod)> {
        match data_type {
            DataType::Text | DataType::Json => {
                // Use Zstd for text-based data
                let compressed = zstd::encode_all(data, 21)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                Ok((compressed, CompressionMethod::Zstd))
            },
            DataType::Image => {
                // For images, attempt to optimize using image crate
                if let Ok(img) = image::load_from_memory(data) {
                    let mut output: Vec<u8> = Vec::new();
                    let mut output = std::io::Cursor::new(Vec::new());
                    img.write_to(&mut output, ImageFormat::WebP).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                    Ok((output.into_inner(), CompressionMethod::None))
                } else {
                    // Fallback to regular compression
                    let compressed = zstd::encode_all(data, 21)
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                    Ok((compressed, CompressionMethod::Zstd))
                }
            },
            DataType::Structured => {
                // Use delta encoding for structured data if possible
                if let Ok(numbers) = bincode::deserialize::<Vec<i64>>(data) {
                    let encoded = self.delta_encode(&numbers);
                    Ok((encoded, CompressionMethod::DeltaEncoding))
                } else {
                    // Fallback to regular compression
                    let compressed = zstd::encode_all(data, 21)
                        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                    Ok((compressed, CompressionMethod::Zstd))
                }
            },
            _ => {
                // Default to Zstd compression
                let compressed = zstd::encode_all(data, 21)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                Ok((compressed, CompressionMethod::Zstd))
            }
        }
    }

    fn delta_encode(&self, numbers: &[i64]) -> Vec<u8> {
        let mut encoded = Vec::with_capacity(numbers.len() * 8);
        if numbers.is_empty() {
            return encoded;
        }

        // Store first number as-is
        encoded.extend_from_slice(&numbers[0].to_le_bytes());

        // Store differences
        for window in numbers.windows(2) {
            let diff = window[1] - window[0];
            encoded.extend_from_slice(&diff.to_le_bytes());
        }

        encoded
    }

    fn write_block(&mut self, block: &Block) -> io::Result<BlockLocation> {
        // Seek to end of file
        self.file.seek(SeekFrom::End(0))?;
        let offset = self.file.stream_position()?;

        // Serialize and write header
        let header_bytes = bincode::serialize(&block.header)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        
        let header_size = header_bytes.len() as u32;
        self.file.write_all(&header_size.to_le_bytes())?;
        self.file.write_all(&header_bytes)?;

        // Write data
        self.file.write_all(&block.data)?;

        Ok(BlockLocation {
            offset,
            header_size,
            data_size: block.data.len() as u64,
        })
    }

    fn read_block(&mut self, location: &BlockLocation) -> io::Result<Block> {
        self.file.seek(SeekFrom::Start(location.offset))?;

        // Read header
        let mut header_size_bytes = [0u8; 4];
        self.file.read_exact(&mut header_size_bytes)?;
        let header_size = u32::from_le_bytes(header_size_bytes);

        let mut header_bytes = vec![0u8; header_size as usize];
        self.file.read_exact(&mut header_bytes)?;

        let header: BlockHeader = bincode::deserialize(&header_bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Read data
        let mut data = vec![0u8; header.compressed_size as usize];
        self.file.read_exact(&mut data)?;

        Ok(Block {
            header,
            data,
            next_location: None, // Will be set if needed
        })
    }

    fn update_metadata(&mut self) -> io::Result<()> {
        let metadata_bytes = bincode::serialize(&self.metadata)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        self.file.seek(SeekFrom::Start(5))?; // After magic bytes and version
        self.file.write_all(&(metadata_bytes.len() as u64).to_le_bytes())?;
        self.file.write_all(&metadata_bytes)?;

        Ok(())
    }
}

#[derive(Debug)]
struct Block {
    header: BlockHeader,
    data: Vec<u8>,
    next_location: Option<BlockLocation>,
}

// Example usage and tests
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_basic_storage() -> io::Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("test.usf");
        
        let mut storage = UniversalStorage::create(&file_path)?;
        
        // Store text
        let text = "Hello, World!".as_bytes();
        storage.store("greeting", text, DataType::Text)?;
        
        // Retrieve text
        let retrieved = storage.retrieve("greeting")?;
        assert_eq!(text, retrieved.as_slice());
        
        Ok(())
    }

    #[test]
    fn test_large_data() -> io::Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("test_large.usf");
        
        let mut storage = UniversalStorage::create(&file_path)?;
        
        // Create large data
        let large_data: Vec<u8> = (0..BLOCK_SIZE * 3).map(|i| (i % 256) as u8).collect();
        
        // Store data
        storage.store("large", &large_data, DataType::Binary)?;
        
        // Retrieve data
        let retrieved = storage.retrieve("large")?;
        assert_eq!(large_data, retrieved);
        
        Ok(())
    }
}