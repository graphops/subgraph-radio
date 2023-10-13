use graphql_client::GraphQLQuery;
#[derive(GraphQLQuery, Copy, Clone, Debug)]
#[graphql(
    schema_path = "schema.json",
    query_path = "src/query/comparison.graphql",
    response_derives = "Debug,Clone,Eq,PartialEq"
)]
pub struct ComparisonQuery;
