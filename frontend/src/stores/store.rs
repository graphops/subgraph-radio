use crate::query::queries::{comparison_query, comparison_query::ResponseData, ComparisonQuery};
use futures::select;
use futures::{FutureExt, TryFutureExt};
use gloo::timers::future::TimeoutFuture;
use graphql_client::reqwest::post_graphql;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use thiserror::Error;
use tokio::time::Duration;
use yewdux::prelude::*;
const SUBGRAPH_RADIO_URL: &str = "http://localhost:3012/api/v1/graphql";
use reqwest::Error as ReqwestError;

#[derive(Debug, PartialEq, Clone)]
pub struct MyReqwestError {
    status: Option<reqwest::StatusCode>,
    url: Option<String>,
    text: String,
}

impl fmt::Display for MyReqwestError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "Reqwest error: status: {:?}, url: {:?}, text: {}",
            self.status, self.url, self.text
        )
    }
}

impl From<ReqwestError> for MyReqwestError {
    fn from(err: ReqwestError) -> Self {
        MyReqwestError {
            status: err.status(),
            url: err.url().map(|u| u.to_string()),
            text: format!("{}", err),
        }
    }
}

impl std::error::Error for MyReqwestError {}

#[derive(Error, Debug, PartialEq, Clone)]
pub enum FetchDataError {
    #[error("Reqwest error: {0}")]
    ReqwestError(#[from] MyReqwestError),
    #[error("Timeout error")]
    TimeoutError,
    #[error("Response data missing")]
    ResponseDataMissing,
}

#[derive(Store, Default, PartialEq, Clone)]
pub struct Store {
    pub indexers_list: IndexersSubstore,
}

#[derive(Default, PartialEq, Clone)]
pub struct IndexersSubstore {
    page: u32,
    per_page: u32,
    pub data: FetchState<comparison_query::ResponseData>,
}

impl Store {
    pub fn refresh_indexers(&mut self) {
        self.indexers_list.data = FetchState::Fetching;

        yew::platform::spawn_local(async move {
            let dispatch = Dispatch::<Store>::new();

            match send_query(SUBGRAPH_RADIO_URL).await {
                Ok(indexers_list) => dispatch.reduce_mut(|state| {
                    state.indexers_list.data = FetchState::Success(indexers_list);
                }),
                Err(error) => dispatch.reduce_mut(|state| {
                    state.indexers_list.data = FetchState::Failed(error);
                }),
            }
        });
    }
}

#[derive(Default, PartialEq, Clone)]
pub enum FetchState<Response> {
    #[default]
    NotFetching,
    Fetching,
    Success(Response),
    Failed(FetchDataError),
}

pub async fn send_query(url: &'static str) -> Result<ResponseData, FetchDataError> {
    let timeout_duration = Duration::from_secs(10);
    let client = reqwest::Client::new();
    let variables = comparison_query::Variables {};
    let request_future = post_graphql::<ComparisonQuery, _>(&client, url, variables)
        .map_err(|e| FetchDataError::ReqwestError(MyReqwestError::from(e)));

    let timeout_future: Pin<Box<dyn Future<Output = Result<(), FetchDataError>>>> = Box::pin(
        TimeoutFuture::new(timeout_duration.as_millis() as u32)
            .map(|_| Err(FetchDataError::TimeoutError)),
    );

    select! {
        res = request_future.fuse() => {
            let data = res?.data.ok_or(FetchDataError::ResponseDataMissing)?;  // Handle None case safely
            Ok(data)
        },
        _ = timeout_future.fuse() => Err(FetchDataError::TimeoutError),
    }
}
