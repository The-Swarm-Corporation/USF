use usf::{UniversalStorage, DataType};

let mut storage = UniversalStorage::create("data.usf")?;
storage.store("key", data, DataType::Text)?;