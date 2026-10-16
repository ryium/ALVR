mod decode_test;
mod probe;

use alvr_common::anyhow::Result;
use std::{path::PathBuf, process};

fn print_usage() {
    println!(
        r#"FrameConnect - SteamOS Native VR Streaming Client

Usage:
  frameconnect                                Run OpenXR streaming lobby and client
  frameconnect probe [OPTIONS]                Probe system, V4L2 decoders, and OpenXR
  frameconnect decode-test <FILE> [OPTIONS]   Run V4L2 hardware decoder verification

Probe Options:
  --output <PATH>     Path to output JSON report

Decode Test Options:
  --frames <COUNT>    Number of frames to decode (default: all)
  --repeat <COUNT>    Repeat the input stream (default: 1)
  --fps <FPS>         Target feed rate in FPS (0 for unthrottled, default: 90)
  --output <PATH>     Path to output JSON report
  --verify-pixels     Verify decoded frame colors

General Options:
  -h, --help          Print this help message
"#
    );
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() <= 1 {
        // Default mode: launch OpenXR client
        alvr_client_openxr::entry_point();
        return Ok(());
    }

    match args[1].as_str() {
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(())
        }
        "probe" => {
            alvr_client_core::init_logging();
            let mut output_path = None;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--output" => {
                        if i + 1 < args.len() {
                            output_path = Some(PathBuf::from(&args[i + 1]));
                            i += 2;
                        } else {
                            eprintln!("Error: --output requires a file path");
                            process::exit(1);
                        }
                    }
                    other => {
                        eprintln!("Unknown probe option: {other}");
                        print_usage();
                        process::exit(1);
                    }
                }
            }
            probe::run_probe(output_path.as_deref())
        }
        "decode-test" => {
            alvr_client_core::init_logging();
            if args.len() < 3 {
                eprintln!("Error: decode-test requires a path to an Annex-B HEVC bitstream");
                print_usage();
                process::exit(1);
            }
            let file_path = PathBuf::from(&args[2]);
            let mut max_frames = None;
            let mut fps = 90.0f32;
            let mut output_path = None;
            let mut verify_pixels = false;
            let mut repeat = 1usize;

            let mut i = 3;
            while i < args.len() {
                match args[i].as_str() {
                    "--repeat" => {
                        repeat = args
                            .get(i + 1)
                            .ok_or_else(|| {
                                alvr_common::anyhow::anyhow!("--repeat requires a count")
                            })?
                            .parse()?;
                        alvr_common::anyhow::ensure!(
                            (1..=10000).contains(&repeat),
                            "--repeat must be between 1 and 10000"
                        );
                        i += 2;
                    }
                    "--frames" => {
                        if i + 1 < args.len() {
                            let count: usize = args[i + 1].parse()?;
                            alvr_common::anyhow::ensure!(count > 0, "--frames must be positive");
                            max_frames = Some(count);
                            i += 2;
                        } else {
                            eprintln!("Error: --frames requires a number");
                            process::exit(1);
                        }
                    }
                    "--fps" => {
                        if i + 1 < args.len() {
                            fps = args[i + 1].parse()?;
                            alvr_common::anyhow::ensure!(
                                fps.is_finite() && (0.0..=1000.0).contains(&fps),
                                "--fps must be finite and between 0 and 1000"
                            );
                            i += 2;
                        } else {
                            eprintln!("Error: --fps requires a floating-point number");
                            process::exit(1);
                        }
                    }
                    "--output" => {
                        if i + 1 < args.len() {
                            output_path = Some(PathBuf::from(&args[i + 1]));
                            i += 2;
                        } else {
                            eprintln!("Error: --output requires a file path");
                            process::exit(1);
                        }
                    }
                    "--verify-pixels" => {
                        verify_pixels = true;
                        i += 1;
                    }
                    other => {
                        eprintln!("Unknown decode-test option: {other}");
                        print_usage();
                        process::exit(1);
                    }
                }
            }

            decode_test::run_decode_test(decode_test::DecodeTestOptions {
                file_path,
                max_frames,
                fps,
                output_path,
                verify_pixels,
                repeat,
            })
        }
        other => {
            eprintln!("Unknown command: {other}");
            print_usage();
            process::exit(1);
        }
    }
}
