use crate::router::{switch, Route};
use yew::prelude::*;
use yew_router::prelude::*;
mod components;
mod pages;
mod query;
mod router;
mod stores;
#[function_component]
fn App() -> Html {
    html! {<BrowserRouter>
        <Switch<Route> render={switch}/>
    </BrowserRouter>}
}
fn main() {
    yew::Renderer::<App>::new().render();
}
