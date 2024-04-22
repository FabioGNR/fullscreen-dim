extern crate ddc_i2c;
extern crate ddc;

use ddc::Ddc;

fn main() {
    let iterator = ddc_i2c::I2cDeviceEnumerator::new().expect("no enumerator");
    println!("Enumerating...");
    for mut ddc in iterator {
        let mccs_version = ddc.get_vcp_feature(0xdf).unwrap();
        println!("MCCS version: {:04x}", mccs_version.maximum());
    }
    println!("Exiting");
}
