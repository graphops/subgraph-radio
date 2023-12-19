use std::collections::HashMap;
use std::time::Instant;

use graphcast_sdk::init_tracing;
use test_runner::sender_validation::{
    id_validation_test_looser_radio_level, id_validation_test_matching_radio_level,
    id_validation_test_stricter_radio_level,
};

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
    init_tracing(config.radio_setup.log_format.to_string()).expect("Could not set up global default subscriber for logger, check environmental variable `RUST_LOG` or the CLI input `log-level");

    let start_time = Instant::now();

    let id_validation_tests = vec![
        (
            "id_validation_test_stricter_radio_level",
            tokio::spawn(id_validation_test_stricter_radio_level()),
        ),
        (
            "id_validation_test_looser_radio_level",
            tokio::spawn(id_validation_test_looser_radio_level()),
        ),
        (
            "id_validation_test_matching_radio_level",
            tokio::spawn(id_validation_test_matching_radio_level()),
        ),
    ];
    let (id_validation_tests_passed, id_validation_test_results) =
        run_tests(id_validation_tests).await;

    // Print test summary
    print_test_summary(
        id_validation_test_results,
        id_validation_tests_passed,
        start_time,
    );
}

fn print_test_summary(
    id_validation_test_results: HashMap<String, bool>, // Simplified parameters
    tests_passed: bool,
    start_time: Instant,
) {
    let elapsed_time = start_time.elapsed();
    // Print summary of tests
    println!("\nTest Summary:\n");
    for (test_name, passed) in &id_validation_test_results {
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
