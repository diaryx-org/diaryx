use serde::Deserialize;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::{IapProduct, IapPurchaseResult, SubscriptionStatus};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_iap);

pub struct Iap<R: Runtime>(PluginHandle<R>);

/// Wrapper for Swift responses that wrap arrays in a `value` key,
/// since `invoke.resolve()` requires a dictionary.
#[derive(Deserialize)]
struct ValueResponse<T> {
    value: T,
}

pub fn init<R: Runtime>(
    _app: &AppHandle<R>,
    api: PluginApi<R, ()>,
) -> Result<Iap<R>, Box<dyn std::error::Error>> {
    #[cfg(target_os = "ios")]
    let handle = api.register_ios_plugin(init_plugin_iap)?;

    #[cfg(target_os = "android")]
    let handle = api.register_android_plugin("com.diaryx.iap", "IapPlugin")?;

    Ok(Iap(handle))
}

impl<R: Runtime> Iap<R> {
    // iOS handlers are async (StoreKit 2). Use the async plugin bridge
    // to avoid invoking async Swift methods through the sync path.
    pub async fn get_products(
        &self,
        product_ids: Vec<String>,
    ) -> Result<Vec<IapProduct>, Box<dyn std::error::Error>> {
        let response: ValueResponse<Vec<IapProduct>> = self
            .0
            .run_mobile_plugin_async(
                "getProducts",
                serde_json::json!({ "productIds": product_ids }),
            )
            .await?;
        Ok(response.value)
    }

    pub async fn purchase(
        &self,
        product_id: String,
        app_account_token: Option<String>,
    ) -> Result<IapPurchaseResult, Box<dyn std::error::Error>> {
        let result: IapPurchaseResult = self
            .0
            .run_mobile_plugin_async(
                "purchase",
                serde_json::json!({
                    "productId": product_id,
                    "appAccountToken": app_account_token,
                }),
            )
            .await?;
        Ok(result)
    }

    pub async fn restore_purchases(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let response: ValueResponse<Vec<String>> = self
            .0
            .run_mobile_plugin_async("restorePurchases", serde_json::json!({}))
            .await?;
        Ok(response.value)
    }

    pub async fn get_subscription_status(
        &self,
        product_id: String,
    ) -> Result<SubscriptionStatus, Box<dyn std::error::Error>> {
        let result: SubscriptionStatus = self
            .0
            .run_mobile_plugin_async(
                "getSubscriptionStatus",
                serde_json::json!({ "productId": product_id }),
            )
            .await?;
        Ok(result)
    }
}
