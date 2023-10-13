use crate::query::queries::comparison_query::ResponseData;
use yew::prelude::*;
#[derive(Properties, PartialEq)]
pub struct Props {
    pub response_data: ResponseData,
}

#[function_component(Table)]
pub fn table(props: &Props) -> Html {
    let subgraphs = props
        .response_data
        .comparison_ratio
        .iter()
        .map(|data_point| {
            html! {

                 <tr>
                    <td>{data_point.clone().deployment}</td>
                    <td>{data_point.clone().block_number}</td>
                    <td>{data_point.clone().sender_ratio}</td>
                    <td>{data_point.clone().stake_ratio}</td>
                </tr>
            }
        })
        .collect::<Html>();
    html! {
    <table class={classes!("table","is-striped","is-bordered", "has-background-danger-light")}>
        <thead>
            <tr>
                <th>{"Deployment"}</th>
                <th>{"block number"}</th>
                <th>{"sender ratio"}</th>
                <th>{"stake ratio"}</th>
            </tr>
        </thead>
        <tbody>
        {subgraphs}
        </tbody>
        </table>
    }
}
