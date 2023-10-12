use crate::pages::home;
use yew::prelude::*;
use yew_router::prelude::*;
#[derive(Clone, Routable, PartialEq)]
pub enum Route {
    #[at("/")]
    Home,
    #[not_found]
    #[at("/404")]
    NotFound,
}

pub fn switch(route: Route) -> Html {
    match route {
        Route::Home => html! {<home::Home />},
        Route::NotFound => html! { <h1>{ "404" }</h1> },
    }
}
