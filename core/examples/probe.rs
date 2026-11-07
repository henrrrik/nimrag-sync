//! Read-only: the mode and version of the first inReach found.
use nimrag::device::{self, InReach};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let found = device::find(std::path::Path::new(device::SYSFS_HIDRAW));
    let first = found.first().ok_or("no inReach on USB")?;
    println!("{first:?}");
    let mut dev = InReach::open(first)?;
    println!("mode: {:?}", dev.app_mode()?);
    println!("{:?}", dev.version()?);
    Ok(())
}
