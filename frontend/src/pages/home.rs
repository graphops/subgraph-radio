use crate::components::table::Table;
use crate::stores::store::FetchState::{self};
use crate::stores::store::Store;

use yew::prelude::*;

use yewdux::prelude::*;
#[function_component(Home)]
pub fn home() -> Html {
    let (store, dispatch) = use_store::<Store>();
    let fetch_unlimited = dispatch.reduce_mut_callback(move |state| state.refresh_indexers());

    let fetch_unlimited2 = fetch_unlimited.clone(); // Clone here
    use_effect_with((), move |_| {
        fetch_unlimited2.emit(());
        || ()
    });

    match &store.indexers_list.data {
        FetchState::NotFetching => html! {
            <>
                <p>{"Doing nothing"}</p>
                <button onclick={Callback::from(move |_event: MouseEvent| fetch_unlimited.clone().emit(()))}>{ "Reload" }</button>
            </>
        },
        FetchState::Fetching => html! { "Fetching" },
        FetchState::Success(response) => html! {
                <div>
                    <button onclick={Callback::from(move |_event: MouseEvent| fetch_unlimited.clone().emit(()))}>{ "Reload" }</button>
                    <Table response_data={response.clone()} />
                </div>
        },
        FetchState::Failed(err) => html! { format!("{:?}", err) },
    }
}
