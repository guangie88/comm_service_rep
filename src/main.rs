#[macro_use]
extern crate derive_new;

#[macro_use]
extern crate error_chain;
extern crate regex;
extern crate reqwest;
extern crate serde;

#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate structopt;

#[macro_use]
extern crate structopt_derive;
extern crate url;

use regex::Regex;
use reqwest::Client;
use std::io::{self, Read, Write};
use std::iter;
use std::process;
use std::thread;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use structopt::StructOpt;
use url::Url;

#[derive(Serialize, Deserialize, Clone, Debug, new)]
#[serde(rename_all = "camelCase")]
struct ExecReq {
    id: String,
    cmd_id_re: String,
    cmd: String,
}

mod errors {
    error_chain! {
        errors {}
    }
}

use errors::*;

#[derive(StructOpt, Debug)]
#[structopt(name = "Comm Service Calling Repeater", about = "Program to repeatedly send command to the given address.")]
struct MainConfig {
    #[structopt(short = "n", default_value = "caller", help = "Name of the caller")]
    name: String,

    #[structopt(short = "r", default_value = ".+", help = "Regex pattern to match the communication service names")]
    regex_pattern: Regex,

    #[structopt(short = "c", help = "Command to run")]
    cmd: String,

    #[structopt(short = "d", help = "Server to send command to")]
    dst_url: Url,

    #[structopt(short = "i", default_value = "1000", help = "Send to interval in milliseconds")]
    interval: u32,
}

fn run() -> Result<()> {
    let config = MainConfig::from_args();
    println!("Config: {:?}", config);

    let interval = Duration::from_millis(config.interval as u64);
    let sync_pair = Arc::new((Mutex::new(false), Condvar::new()));
    let sync_pair_child = sync_pair.clone();

    let child = thread::spawn(move || {
        let &(ref m, ref cv) = &*sync_pair_child;

        iter::repeat(())
            .any(|_| {
                let match_fn = || -> Result<bool> {
                    let guard = match m.lock() {
                        Ok(guard) => guard,
                        Err(e) => bail!("Unable to get mutex lock in thread: {}", e),
                    };

                    let (guard, _) = match cv.wait_timeout(guard, interval) {
                        Ok(cv_res) => cv_res,
                        Err(e) => bail!("Unable to wait for condvar timeout: {}", e),
                    };
                    
                    let is_interrupted = *guard;
                    println!("Is interrupted: {}", is_interrupted);

                    Ok(is_interrupted)
                };

                match match_fn() {
                    // not interrupted
                    Ok(false) => {
                        // sends command here in a separate thread to preserve timing
                        let dst_url = config.dst_url.clone();
                        let name = config.name.clone();
                        let regex_pattern = config.regex_pattern.to_string();
                        let cmd = config.cmd.clone();

                        // detach the HTTP client thread
                        thread::spawn(move || {
                            let client_fn = || -> Result<String> {
                                let client = match Client::new() {
                                    Ok(client) => client,
                                    Err(e) => bail!("Error creating HTTP client: {}", e),
                                };

                                let res = client.post(dst_url)
                                    .json(&ExecReq::new(name, regex_pattern, cmd))
                                    .send();

                                match res {
                                    Ok(mut resp) => {
                                        if resp.status().is_success() {
                                            let mut content = String::new();
                                            let _ = resp.read_to_string(&mut content);

                                            Ok(format!("Success in sending command, body: {} ", content))
                                        } else {
                                            bail!("Success in sending command, but returned status code: {:?}", resp.status());
                                        }
                                    },

                                    Err(e) => {
                                        bail!("Failed to send command: {}", e);
                                    },
                                }
                            };

                            match client_fn() {
                                Ok(msg) => println!("{}", msg),
                                Err(e) => println!("HTTP thread error: {}", e),
                            }
                        });

                        false
                    },

                    Err(e) => {
                        println!("Thread error: {}", e);
                        false
                    },

                    // interrupted
                    _ => true,
                }
            });
    });

    // main thread blocking until something is entered into buffer
    println!("Press [ENTER] to terminate...");

    let mut buf = String::new();

    io::stdin().read_line(&mut buf)
        .chain_err(|| "Unable to read into buffer")?;

    println!("Terminating...");
    let &(ref m, ref cv) = &*sync_pair;

    {
        // must scope to lock as little as possible
        match m.lock() {
            Ok(mut guard) => {
                *guard = true;
            },

            Err(e) => bail!("Unable to get mutex lock in main thread: {}", e),
        }
    }

    cv.notify_one();
    println!("Waiting for child thread to terminate...");

    if let Err(e) = child.join() {
        println!("Error joining child thread: {:?}", e);
    }

    Ok(())
}

fn main() {
    match run() {
        Ok(_) => {
            println!("Program completed!");
            process::exit(0)
        },

        Err(ref e) => {
            let stderr = &mut io::stderr();

            writeln!(stderr, "Error: {}", e)
                .expect("Unable to write error into stderr!");

            for e in e.iter().skip(1) {
                writeln!(stderr, "- Caused by: {}", e)
                    .expect("Unable to write error causes into stderr!");
            }

            process::exit(1);
        },
    }
}
