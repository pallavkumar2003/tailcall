use std::sync::Arc;

use mini_v8::{Invocation, Value, Values};

use crate::channel::{JsRequest, JsResponse};
use crate::cli::javascript::serde_v8::SerdeV8;
use crate::cli::javascript::sync_v8::{SyncV8, SyncV8Function};
use crate::{HttpIO, ToAnyHow};

pub const FETCH: &str = "__fetch__";
pub async fn init(sync_v8: &SyncV8, http: Arc<dyn HttpIO>) -> anyhow::Result<()> {
    let sync_v8 = sync_v8.clone();
    sync_v8
        .clone()
        .borrow(move |v8| {
            let fetch = v8.create_function(move |invocation| {
                let sync_v8 = sync_v8.clone();
                let http: Arc<dyn HttpIO> = http.clone();
                let args = JSFetchArgs::try_from(&sync_v8, &invocation).map_err(|_| {
                    mini_v8::Error::ToJsConversionError {
                        from: "MiniV8 Invocation",
                        to: "JSFetchArgs",
                    }
                })?;
                sync_v8
                    .current()
                    .spawn(async move { fetch(sync_v8, http, args).await });
                Ok(mini_v8::Value::Undefined)
            });
            v8.global()
                .set(FETCH, fetch)
                .or_anyhow(format!("Could not set {} in global v8 object", FETCH).as_str())?;

            Ok(())
        })
        .await
}

#[derive(Clone)]
struct JSFetchArgs {
    request: JsRequest,
    callback: Arc<SyncV8Function>,
}

impl JSFetchArgs {
    fn try_from(sync_v8: &SyncV8, value: &Invocation) -> anyhow::Result<Self> {
        let request = JsRequest::from_v8(&value.args.get(0))?;

        let callback = value.args.get(1).as_function().cloned();
        let callback = callback.ok_or(anyhow::anyhow!(
            "Second argument to fetch must be a function"
        ))?;

        Ok(Self {
            request,
            callback: Arc::new(sync_v8.as_sync_function(callback)),
        })
    }
}

async fn fetch(sync_v8: SyncV8, http: Arc<dyn HttpIO>, args: JSFetchArgs) -> anyhow::Result<()> {
    let request = reqwest::Request::try_from(args.request)?;
    let response = sync_v8
        .current()
        .spawn(async move { http.clone().execute(request).await })
        .await?;
    match response {
        Ok(response) => {
            let js_response = JsResponse::try_from(&response)?;
            let response = serde_json::to_value(js_response)?;
            sync_v8
                .clone()
                .borrow(move |mv8| {
                    args.callback.call::<Value>(Values::from_iter(vec![
                        mini_v8::Value::Null,
                        response.to_v8(mv8)?,
                    ]))?;
                    Ok(())
                })
                .await?;
        }
        Err(e) => {
            let error = e.to_string();
            sync_v8
                .clone()
                .borrow(move |mv8| {
                    let error = error.clone();
                    args.callback.call::<Value>(Values::from_iter(vec![
                        mini_v8::Value::String(mv8.create_string(error.as_str())),
                        mini_v8::Value::Null,
                    ]))?;
                    Ok(())
                })
                .await?;
        }
    };
    Ok(())
}
