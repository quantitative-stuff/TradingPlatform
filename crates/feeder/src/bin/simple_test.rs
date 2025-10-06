fn main() {
    println!("Hello, this is a simple test!");
    println!("Environment variable EXCHANGES: {:?}", std::env::var("EXCHANGES"));
    println!("Current directory: {:?}", std::env::current_dir());
    println!("Test completed successfully!");
}