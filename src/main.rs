extern crate ddc_i2c;
extern crate ddc;
extern crate udev;
extern crate i2c_linux;
extern crate edid;
extern crate libwmctl;

use clap::Parser;
use i2c_linux::I2c;
use udev::Device;
use xrandr::Monitor;

use crate::ddc::Edid;
use std::{os::unix::ffi::OsStrExt, rc::Rc};

use ddc::{Ddc, FeatureCode};
use ddc_i2c::I2cDdc;
use edid::{Descriptor, EDID};
use libwmctl::WmCtl;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Displays to ignore (displays that match this enum are not dimmed)
    #[arg(short, long)]
    ignore: Vec<String>,

    /// Apps to ignore (apps containing this string in name are skipped)
    #[arg(long)]
    ignore_apps: Vec<String>,

    /// Only look at the focused app
    #[arg(long)]
    focused_only: bool,

    /// Fade time in milliseconds
    #[arg(short, long)]
    fade_time: Option<u64>,

    /// Poll interval in milliseconds (time to wait between checking for fullscreen apps)
    #[arg(long)]
    poll_interval: Option<u64>,
    
    /// Fade interval in milliseconds (time to wait between sending brightness updates)
    #[arg(long)]
    fade_interval: Option<u64>,
}

const BRIGHTNESS: FeatureCode = 0x10;

#[derive(PartialEq, Debug, Clone)]
struct Geometry(i32,i32,i32,i32);

impl Geometry {
    fn new(x: i32, y: i32, width: i32, height: i32) -> Geometry {
        Geometry(x, y, width, height)
    }

    fn from_window(x: i32, y: i32, width: u32, height: u32) -> Geometry {
        Geometry(x, y, width as i32, height as i32)
    }
}

struct Application {
    geometry: Geometry,
    name: Option<String>
}

struct Screen {
    ddc: I2cDdc<I2c<std::fs::File>>,
    default_brightness: u16,
    current_brightness: u16,
    name: String,
    edid: EDID,
    monitor: Option<Rc<X11Monitor>>
}

impl Screen {
    fn new(i2c: I2c<std::fs::File>, device: &Device) -> Option<Screen> {
        let i2c_name = match device.attribute_value("name") {
            Some(v) => v,
            None => return None
        };

        // list stolen from ddcutil's ignorable_i2c_device_sysfs_name
        let skip_prefix = ["SMBus", "soc:i2cdsi", "smu", "mac-io", "u4"];

        if skip_prefix.iter().any(|p| i2c_name.as_bytes().starts_with(p.as_bytes())) {
            return None;
        }

        let mut ddc = I2cDdc::new(i2c);

        let mut edid_data = [0u8; 0x80];
        if ddc.read_edid(0, &mut edid_data).is_err() {
            return None;
        }

        let brightness_value = ddc.get_vcp_feature(BRIGHTNESS);
        let current_brightness = brightness_value.as_ref().map_or(0, |v| v.value());
        let maximum_brightness = brightness_value.as_ref().map_or(100, |v| v.value());

        let edid = edid::parse(&edid_data).unwrap();
        for descriptor in &edid.1.descriptors {
            match descriptor {
                Descriptor::ProductName(product_name) => {
                    return Some(Screen {
                        ddc,
                        default_brightness: if product_name == "DM-163BD-MPGR" { 50 } else { maximum_brightness },
                        current_brightness,
                        name: product_name.to_owned(),
                        edid: edid.1,
                        monitor: None
                    });
                },
                _ => continue
            }
        }

        None
    }

    fn set_brightness(&mut self, brightness: u16) {
        self.ddc.set_vcp_feature(BRIGHTNESS, brightness).unwrap();
        self.current_brightness = brightness;
    }
}

struct X11Monitor {
    geometry: Geometry,
    outputs: Vec<EDID>
}

impl X11Monitor {
    fn new(monitor: &Monitor) -> X11Monitor {
        let outputs = monitor.outputs.iter()
            .filter_map(|o| o.edid())
            .filter_map(|e| edid::parse(e.as_slice()).to_result().ok())
            .collect();

        X11Monitor {
            geometry: Geometry::new(monitor.x, monitor.y, monitor.width_px, monitor.height_px),
            outputs
        }
    }

    fn matches_window_geometry(&self, geometry: &Geometry) -> bool {
        self.geometry == *geometry
    }

    fn has_edid(&self, edid: &EDID) -> bool {
        self.outputs.contains(&edid)
    }
}

fn get_compatible_screens() -> Result<impl Iterator<Item = Screen>, std::io::Error> {
    let enumerator = i2c_linux::Enumerator::new()?;
    
    Ok(enumerator.filter_map(|x| Screen::new(x.0, &x.1)))
}

fn is_eligible_fullscreen_window(wmctl: &WmCtl, window: u32, ignore_apps: &Vec<String>) ->  Result<Option<Application>, libwmctl::ErrorWrapper> {
    let name = wmctl.win_name(window);
    if let Ok(name) = &name {
        for ignore in ignore_apps {
            if name.contains(ignore) {
                return Ok(None);
            }
        }
    } else {
        return Ok(None);
    }

    let states = wmctl.win_state(window);
    if let Ok(states) = states {
        if states.contains(&libwmctl::WinState::Fullscreen) {
            let geometry = wmctl.win_geometry(window)?;
            let geometry = Geometry::from_window(geometry.0, geometry.1, geometry.2, geometry.3);
            return Ok(Some(Application { geometry, name: name.ok() }));
        }
    }

    Ok(None)
}

fn get_fullscreen_app(wmctl: &WmCtl, ignore_apps: &Vec<String>, focused_only: bool) -> Result<Option<Application>, libwmctl::ErrorWrapper> {
    if focused_only {
        let active_window = wmctl.active_win()?;
        return is_eligible_fullscreen_window(wmctl, active_window, ignore_apps);
    }

    for win in wmctl.windows(false)? {
        let fullscreen_app = is_eligible_fullscreen_window(wmctl, win, ignore_apps);
        if let Ok(fullscreen_geometry) = fullscreen_app {
            if fullscreen_geometry.is_some() {
                return Ok(fullscreen_geometry);
            }
        }
    }
    Ok(None)
}

fn is_screen_fullscreen(app_geometry: Option<&Geometry>, monitor: Option<&X11Monitor>) -> bool {
    if let Some(app_geometry) = app_geometry {
        if let Some(monitor) = monitor {
            return monitor.matches_window_geometry(app_geometry);
        }
    }
    return false;
}

struct FadeTiming {
    duration: std::time::Duration,
    interval: std::time::Duration
}

fn main() {
    let args = Args::parse();

    let fade_timing = {
        let duration = std::time::Duration::from_millis(args.fade_time.unwrap_or(1000));
        let interval = std::time::Duration::from_millis(args.fade_interval.unwrap_or(10));
        FadeTiming { duration, interval } 
    };

    let poll_interval = std::time::Duration::from_millis(args.poll_interval.unwrap_or(500));

    let mut screens_to_fade = get_screens(&args);

    let mut last_fullscreen_geometry: Option<Geometry> = None;
    let wmctl = WmCtl::connect().unwrap();

    loop {
        let fullscreen_app = get_fullscreen_app(&wmctl, &args.ignore_apps, args.focused_only).unwrap();

        let fade_out = fullscreen_app.is_some();
        let geometry = fullscreen_app.as_ref().and_then(|a| Some(a.geometry.clone()));
        if geometry != last_fullscreen_geometry {
            if let Some(application) = fullscreen_app {
                println!("Fading out due to app {}", application.name.unwrap_or("unknown".to_owned()));
            } else {
                println!("Fading back in");
            }
            fade(fade_out, &mut screens_to_fade, &geometry, &fade_timing);
            last_fullscreen_geometry = geometry;
        }
        std::thread::sleep(poll_interval);
    }
}

fn get_screens(args: &Args) -> Vec<Screen> {
    let x11_monitors: Vec<Rc<X11Monitor>> = {
        let mut handle = xrandr::XHandle::open().unwrap();
        handle.monitors().unwrap().iter().map(X11Monitor::new).map(Rc::new).collect()
    };

    let mut screens_to_fade: Vec<Screen> = get_compatible_screens().unwrap()
        .filter(|s| !args.ignore.contains(&s.name)).collect();

    for screen in &mut screens_to_fade {
        println!("Screen {:?}, default {:?}, current {:?}", screen.name, screen.default_brightness, screen.current_brightness);
        let monitor = x11_monitors.iter().find(|m| m.has_edid(&screen.edid));
        screen.monitor = monitor.cloned();
    }
    screens_to_fade
}

fn fade(fade_out: bool, screens_to_fade: &mut Vec<Screen>, geometry: &Option<Geometry>, timing: &FadeTiming) {
    let start_time = std::time::Instant::now();
    loop {
        let now = std::time::Instant::now();
        let elapsed = now - start_time;
        let progress = elapsed.as_secs_f64() / timing.duration.as_secs_f64();

        for screen in screens_to_fade.iter_mut() {
            let is_fullscreen = is_screen_fullscreen(geometry.as_ref(), screen.monitor.as_deref());
            let fade_out_screen = fade_out && !is_fullscreen;
            if (fade_out_screen && screen.current_brightness == 0) || (!fade_out_screen && screen.current_brightness >= screen.default_brightness) {
                continue;
            }
            let brightness = if fade_out_screen {
                (1.0 - progress) * screen.default_brightness as f64
            } else { 
                progress * screen.default_brightness as f64 
            };
        
            screen.set_brightness(brightness as u16);
        }

        if progress >= 1.0 {
            break;
        }
    
        std::thread::sleep(timing.interval);
    }
}
