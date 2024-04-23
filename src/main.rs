extern crate ddc_i2c;
extern crate ddc;
extern crate udev;
extern crate i2c_linux;
extern crate edid;

use clap::Parser;

use crate::ddc::Edid;
use std::os::unix::ffi::OsStrExt;

use ddc::{Ddc, FeatureCode};
use ddc_i2c::I2cDdc;
use edid::Descriptor;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Name of the display to change
    #[arg(index(1), required(true))]
    display_name: String,

    /// Brightness to set
    #[arg(short, long)]
    brightness: u16,
    
    /// Start brightness to fade from
    #[arg(short, long)]
    start_brightness: Option<u16>,

    /// Use fade effect
    #[arg(short, long, action)]
    fade: bool,
}

const BRIGHTNESS: FeatureCode = 0x10;

fn main() {
    let args = Args::parse();

    let enumerator = i2c_linux::Enumerator::new().unwrap();
    for (i2c, dev) in enumerator {
        let name = match dev.attribute_value("name") {
            Some(v) => v,
            None => continue,
        };

        // list stolen from ddcutil's ignorable_i2c_device_sysfs_name
        let skip_prefix = ["SMBus", "soc:i2cdsi", "smu", "mac-io", "u4"];

        if skip_prefix.iter().any(|p| name.as_bytes().starts_with(p.as_bytes())) {
            continue;
        }

        let mut ddc = I2cDdc::new(i2c);

        let mut edid_data = [0u8; 0x80];
        if ddc.read_edid(0, &mut edid_data).is_err() {
            continue;
        }

        // for attribute in dev.attributes() {
            //     println!("\t {:?} = {:?}", attribute.name(), attribute.value());
            // }
            
        let edid = edid::parse(&edid_data).unwrap();
        let mut name: Option<String> = None;
        for descriptor in edid.1.descriptors {
            match descriptor {
                Descriptor::ProductName(product_name) => {
                    name = Some(product_name);
                },
                _ => continue
            }
        }

        println!("Device {:?} with name {:?}", dev.devnode(), name.as_ref().unwrap_or(&"unknown".to_string()));

        if let Some(name) = name {
            if name == args.display_name {
                let brightness = ddc.get_vcp_feature(BRIGHTNESS).unwrap();
                println!("\tBrightness: {:04x}", brightness.value());
                if args.fade {
                    println!("fading");
                    let mut brightness = args.start_brightness.unwrap_or(brightness.value());
                    while args.brightness != brightness {
                        if brightness > args.brightness {
                            brightness -= 1;
                        } else {
                            brightness += 1;
                        }
                        ddc.set_vcp_feature(BRIGHTNESS, brightness).unwrap();
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                } else {
                    ddc.set_vcp_feature(BRIGHTNESS, args.brightness).unwrap();
                }
            }
            break;
        }
    }
}
