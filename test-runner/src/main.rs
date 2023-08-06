use std::collections::HashMap;
use std::time::Instant;

use graphcast_sdk::init_tracing;
use test_runner::message_handling::send_and_receive_test;
use test_utils::config::test_config;
use tracing::{error, info};

async fn run_tests(
    tests: Vec<(&str, tokio::task::JoinHandle<()>)>,
) -> (bool, HashMap<String, bool>) {
    let mut tests_passed = true;
    let mut test_results: HashMap<String, bool> = HashMap::new();

    for (test_name, test_task) in tests {
        match test_task.await {
            Ok(()) => {
                info!("{} passed ✅", test_name);
                test_results.insert(test_name.to_string(), true);
            }
            Err(_e) => {
                error!("{} failed ❌", test_name);
                tests_passed = false;
                test_results.insert(test_name.to_string(), false);
            }
        }
    }

    (tests_passed, test_results)
}
#[tokio::main]
pub async fn main() {
    let config = test_config();

    std::env::set_var(
            "RUST_LOG",
            "off,hyper=off,graphcast_sdk=trace,subgraph_radio=trace,test_runner=trace,test_sender=trace,test_utils=trace",
    );
    init_tracing(config.log_format).expect("Could not set up global default subscriber for logger, check environmental variable `RUST_LOG` or the CLI input `log-level");

    let start_time = Instant::now();

    // Only run the send_and_receive_test, without any retry logic.
    let single_test = vec![(
        "send_and_receive_test",
        tokio::spawn(send_and_receive_test()),
    )];

    let (test_passed, test_results) = run_tests(single_test).await;

    // Here we only print the result for the single test we ran.
    print_test_summary(
        test_results,
        HashMap::new(),
        HashMap::new(),
        HashMap::new(),
        test_passed,
        start_time,
    );
}

fn print_test_summary(
    initial_test_results: HashMap<String, bool>,
    poi_test_results: HashMap<String, bool>,
    validity_test_results_group_1: HashMap<String, bool>,
    validity_test_results_group_2: HashMap<String, bool>,
    tests_passed: bool,
    start_time: Instant,
) {
    let elapsed_time = start_time.elapsed();
    // Print summary of tests
    println!("\nTest Summary:\n");
    for (test_name, passed) in initial_test_results
        .iter()
        .chain(&poi_test_results)
        .chain(&validity_test_results_group_1)
        .chain(&validity_test_results_group_2)
    {
        if *passed {
            info!("{}: PASSED", test_name);
        } else {
            error!("{}: FAILED", test_name);
        }
    }

    if tests_passed {
        info!(
            "All tests passed ✅. Time elapsed: {}s",
            elapsed_time.as_secs()
        );
    } else {
        error!(
            "Some tests failed ❌. Time elapsed: {}s",
            elapsed_time.as_secs()
        );
    }
}
