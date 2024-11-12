use std::io::{self, Write};
use std::fs;
use chrono::Utc;
use log::{info, error};
use simplelog::{Config, LevelFilter, SimpleLogger};
use usf::{UniversalStorage, DataType};

fn main() -> io::Result<()> {
    // Initialize logging
    SimpleLogger::init(LevelFilter::Info, Config::default()).unwrap();
    info!("Starting Universal Storage Feature Demonstration");

    // File setup
    let path = "demo_storage.usf";
    if fs::metadata(path).is_ok() {
        fs::remove_file(path)?; // Remove file if it exists
    }

    // Create UniversalStorage instance
    let mut storage = UniversalStorage::create(path)?;
    info!("Created new Universal Storage file at {}", path);

    // Store and retrieve text data
    let text_data = "Hello, Universal Storage!".as_bytes();
    storage.store("sample_text", text_data, DataType::Text)?;
    info!("Stored text data with key 'sample_text'");

    let retrieved_text = storage.retrieve("sample_text")?;
    assert_eq!(retrieved_text, text_data);
    info!("Retrieved text data: {:?}", String::from_utf8(retrieved_text).unwrap());

    // Store and retrieve binary data
    let binary_data = vec![1, 2, 3, 4, 5, 6, 7, 8, 9];
    storage.store("binary_data", &binary_data, DataType::Binary)?;
    info!("Stored binary data with key 'binary_data'");

    let retrieved_binary = storage.retrieve("binary_data")?;
    assert_eq!(retrieved_binary, binary_data);
    info!("Retrieved binary data: {:?}", retrieved_binary);

    // Store and retrieve large binary data
    let large_data: Vec<u8> = (0..1024 * 3).map(|i| (i % 256) as u8).collect(); // 3KB
    storage.store("large_data", &large_data, DataType::Binary)?;
    info!("Stored large binary data with key 'large_data'");

    let retrieved_large_data = storage.retrieve("large_data")?;
    assert_eq!(retrieved_large_data, large_data);
    info!("Retrieved large data successfully, size: {} bytes", retrieved_large_data.len());

    // Store and retrieve JSON data
    let json_data = r#"{"name": "Universal Storage", "type": "File Format"}"#.as_bytes();
    storage.store("json_data", json_data, DataType::Json)?;
    info!("Stored JSON data with key 'json_data'");

    let retrieved_json = storage.retrieve("json_data")?;
    assert_eq!(retrieved_json, json_data);
    info!("Retrieved JSON data: {:?}", String::from_utf8(retrieved_json).unwrap());

    // Clean up: remove demo storage file after demonstration
    if fs::remove_file(path).is_ok() {
        info!("Demo storage file removed successfully after testing.");
    } else {
        error!("Failed to remove demo storage file.");
    }

    Ok(())
}
