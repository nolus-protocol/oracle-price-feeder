use time::{format_description::well_known::Iso8601, OffsetDateTime};

fn main() {
    println!(
        "cargo:rustc-env=BUILD_START_TIME_DATE={}",
        OffsetDateTime::now_utc()
            .format(&Iso8601::DEFAULT)
            .expect("Couldn't format system's time and date!")
    );
}
