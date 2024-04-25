# Dims monitors when a fullscreen window (X11) exists using DDC/CI

Runs as a background application that periodically checks for fullscreen apps, and either dims or brightens monitors.  
Currently only X11 is supported to check for fullscreen applications.

## Usage

    Usage: fullscreen-dim [OPTIONS]

    Options:
    -i, --ignore <IGNORE>                Displays to ignore (displays that match this enum are not dimmed)
        --ignore-apps <IGNORE_APPS>      Apps to ignore (apps containing this string in name are skipped)
    -f, --fade-time <FADE_TIME>          Fade time in milliseconds
        --poll-interval <POLL_INTERVAL>  Poll interval in milliseconds (time to wait between checking for fullscreen apps)
        --fade-interval <FADE_INTERVAL>  Fade interval in milliseconds (time to wait between sending brightness updates)
    -h, --help                           Print help
    -V, --version                        Print version

Example: `fullscreen-dim --ignore 2590G4 --ignore-apps Discord --fade-time 1500`