use std::sync::Arc;

use async_graphql::dynamic;
use derive_setters::Setters;

use super::HttpClient;
use crate::blueprint::Type::ListType;
use crate::blueprint::{Blueprint, Definition};
use crate::http::{GraphqlDataLoader, HttpDataLoader};
use crate::lambda::{Expression, Unsafe};

#[derive(Setters, Clone)]
pub struct ServerContext {
  pub schema: dynamic::Schema,
  pub http_client: Arc<dyn HttpClient>,
  pub blueprint: Blueprint,
}

fn assign_data_loaders(blueprint: &mut Blueprint, http_client: Arc<dyn HttpClient>) -> &Blueprint {
  for def in blueprint.definitions.iter_mut() {
    if let Definition::ObjectTypeDefinition(def) = def {
      for field in &mut def.fields {
        if let Some(Expression::Unsafe(Unsafe::Http(req_template, group_by, _))) = &mut field.resolver {
          let data_loader = HttpDataLoader::new(
            http_client.clone(),
            group_by.clone(),
            matches!(&field.of_type, ListType { .. }),
          )
          .to_data_loader(blueprint.upstream.batch.clone().unwrap_or_default());
          field.resolver = Some(Expression::Unsafe(Unsafe::Http(
            req_template.clone(),
            group_by.clone(),
            Some(Arc::new(data_loader)),
          )));
        }
        if let Some(Expression::Unsafe(Unsafe::GraphQLEndpoint(req_template, field_name, use_batch_request, _))) =
          &mut field.resolver
        {
          let graphql_data_loader = GraphqlDataLoader::new(http_client.clone(), *use_batch_request)
            .to_data_loader(blueprint.upstream.batch.clone().unwrap_or_default());
          field.resolver = Some(Expression::Unsafe(Unsafe::GraphQLEndpoint(
            req_template.clone(),
            field_name.clone(),
            *use_batch_request,
            Some(Arc::new(graphql_data_loader)),
          )))
        }
      }
    }
  }
  blueprint
}

impl ServerContext {
  pub fn new(blueprint: Blueprint, http_client: Arc<dyn HttpClient>) -> Self {
    let schema = assign_data_loaders(&mut blueprint.clone(), http_client.clone()).to_schema();
    ServerContext { schema, http_client, blueprint }
  }
}
