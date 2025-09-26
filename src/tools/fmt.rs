pub fn human_readable_size(n: u64) -> String {
    let mut f = n as f64;
    let mut div = 0;
    while f > 1024.0 {
        f /= 1024.0;
        div += 1;
    }
    match div {
        0 => format!("{:.0} bytes", f),
        1 => format!("{:04.2} KiB", f),
        2 => format!("{:04.2} MiB", f),
        3 => format!("{:04.2} GiB", f),
        4 => format!("{:04.2} TiB", f),
        _ => String::from("too many bytes"),
    }
}

pub fn human_readable_duration(n: u64) -> String {
    let mut seconds = n;
    let hours = seconds / 60 / 60;
    seconds -= hours * 60 * 60;
    let min = seconds / 60;
    seconds -= min * 60;
    format!("{:02}h{:02}m{:02}s", hours, min, seconds)
}
