use std::collections::HashMap;
use std::time::Instant;

use graphcast_sdk::init_tracing;
use test_runner::{
    invalid_block_hash::invalid_block_hash_test, invalid_nonce::invalid_nonce_test,
    invalid_payload::invalid_payload_test, invalid_sender::invalid_sender_test,
    message_handling::send_and_receive_test, poi_divergent::poi_divergent_test,
    poi_match::poi_match_test, topics::topics_test,
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

    // Run send_and_receive_test separately
    let send_and_receive_tests = vec![(
        "send_and_receive_test",
        tokio::spawn(send_and_receive_test()),
    )];
    let (send_and_receive_tests_passed, send_and_receive_test_results) =
        run_tests(send_and_receive_tests).await;

    // Run topics_test separately
    let topics_tests = vec![("topics_test", tokio::spawn(topics_test()))];
    let (topics_tests_passed, topics_test_results) = run_tests(topics_tests).await;

    // Run poi_divergent_test separately
    let poi_divergent_tests = vec![("poi_divergent_test", tokio::spawn(poi_divergent_test()))];
    let (poi_divergent_tests_passed, poi_divergent_test_results) =
        run_tests(poi_divergent_tests).await;

    // Run poi_match_test separately
    let poi_match_tests = vec![("poi_match_test", tokio::spawn(poi_match_test()))];
    let (poi_match_tests_passed, poi_match_test_results) = run_tests(poi_match_tests).await;

    // Run other validity tests
    let validity_tests_group_1 = vec![
        (
            "invalid_block_hash_test",
            tokio::spawn(invalid_block_hash_test()),
        ),
        ("invalid_sender_test", tokio::spawn(invalid_sender_test())),
    ];
    let (validity_tests_group_1_passed, validity_test_results_group_1) =
        run_tests(validity_tests_group_1).await;

    let validity_tests_group_2 = vec![
        ("invalid_nonce_test", tokio::spawn(invalid_nonce_test())),
        ("invalid_payload_test", tokio::spawn(invalid_payload_test())),
    ];
    let (validity_tests_group_2_passed, validity_test_results_group_2) =
        run_tests(validity_tests_group_2).await;

    // Print test summary
    print_test_summary(
        send_and_receive_test_results,
        topics_test_results,
        poi_divergent_test_results,
        poi_match_test_results,
        validity_test_results_group_1,
        validity_test_results_group_2,
        send_and_receive_tests_passed
            && topics_tests_passed
            && poi_divergent_tests_passed
            && poi_match_tests_passed
            && validity_tests_group_1_passed
            && validity_tests_group_2_passed,
        start_time,
    );
}

#[allow(clippy::too_many_arguments)]
fn print_test_summary(
    send_and_receive_test_results: HashMap<String, bool>,
    topics_test_results: HashMap<String, bool>,
    poi_divergent_test_results: HashMap<String, bool>,
    poi_match_test_results: HashMap<String, bool>,
    validity_test_results_group_1: HashMap<String, bool>,
    validity_test_results_group_2: HashMap<String, bool>,
    tests_passed: bool,
    start_time: Instant,
) {
    let elapsed_time = start_time.elapsed();
    // Print summary of tests
    println!("\nTest Summary:\n");
    for (test_name, passed) in send_and_receive_test_results
        .iter()
        .chain(&topics_test_results)
        .chain(&poi_divergent_test_results)
        .chain(&poi_match_test_results)
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
