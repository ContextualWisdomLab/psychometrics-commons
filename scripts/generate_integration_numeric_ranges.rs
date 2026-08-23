//! Generate the PostgreSQL integer ranges that mirror Rust's `char::is_numeric` set.
//!
//! Compile and run this file with the repository-pinned Rust toolchain whenever that
//! toolchain changes. The printed `int4multirange` literal can then be compared with
//! `migrations/0001_integration_delivery.sql` before accepting the toolchain update.

fn main() {
    let mut ranges = Vec::new();
    let mut range_start = None;
    let mut previous_numeric = 0_u32;

    for codepoint in 0..=char::MAX as u32 {
        let is_numeric = char::from_u32(codepoint).is_some_and(char::is_numeric);
        match (range_start, is_numeric) {
            (None, true) => {
                range_start = Some(codepoint);
                previous_numeric = codepoint;
            }
            (Some(_), true) => previous_numeric = codepoint,
            (Some(start), false) => {
                ranges.push((start, previous_numeric + 1));
                range_start = None;
            }
            (None, false) => {}
        }
    }

    if let Some(start) = range_start {
        ranges.push((start, previous_numeric + 1));
    }

    print!("{{");
    for (index, (start, end)) in ranges.iter().enumerate() {
        if index > 0 {
            print!(",");
        }
        print!("[{start},{end})");
    }
    println!("}}");
}
