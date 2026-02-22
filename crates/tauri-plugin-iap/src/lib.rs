use serde::{Deserialize, Serialize};
#[cfg(mobile)]
use tauri::Manager;
use tauri::{
    plugin::{Builder, TauriPlugin},
    AppHandle, Runtime,
};

#[cfg(mobile)]
mod mobile;

// --- Models ---

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IapProduct {
    pub id: String,
    pub title: String,
    pub description: String,
    pub price: String,
    pub price_locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IapPurchaseResult {
    pub transaction_id: String,
    pub original_transaction_id: String,
    pub product_id: String,
    pub signed_transaction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionStatus {
    pub is_subscribed: bool,
}

// --- Commands ---

#[tauri::command]
async fn get_products<R: Runtime>(
    app: AppHandle<R>,
    product_ids: Vec<String>,
) -> Result<Vec<IapProduct>, String> {
    #[cfg(mobile)]
    {
        app.state::<mobile::Iap<R>>()
            .get_products(product_ids)
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = (app, product_ids);
        Err("In-app purchases are only available on iOS".into())
    }
}

#[tauri::command]
async fn purchase<R: Runtime>(
    app: AppHandle<R>,
    product_id: String,
    app_account_token: Option<String>,
) -> Result<IapPurchaseResult, String> {
    #[cfg(mobile)]
    {
        app.state::<mobile::Iap<R>>()
            .purchase(product_id, app_account_token)
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = (app, product_id, app_account_token);
        Err("In-app purchases are only available on iOS".into())
    }
}

#[tauri::command]
async fn restore_purchases<R: Runtime>(app: AppHandle<R>) -> Result<Vec<String>, String> {
    #[cfg(mobile)]
    {
        app.state::<mobile::Iap<R>>()
            .restore_purchases()
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = app;
        Err("In-app purchases are only available on iOS".into())
    }
}

#[tauri::command]
async fn get_subscription_status<R: Runtime>(
    app: AppHandle<R>,
    product_id: String,
) -> Result<SubscriptionStatus, String> {
    #[cfg(mobile)]
    {
        app.state::<mobile::Iap<R>>()
            .get_subscription_status(product_id)
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(mobile))]
    {
        let _ = (app, product_id);
        Err("In-app purchases are only available on iOS".into())
    }
}

// --- Plugin init ---

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("iap")
        .invoke_handler(tauri::generate_handler![
            get_products,
            purchase,
            restore_purchases,
            get_subscription_status,
        ])
        .setup(|app, api| {
            #[cfg(mobile)]
            {
                let iap = mobile::init(app, api)?;
                app.manage(iap);
            }
            #[cfg(not(mobile))]
            {
                let _ = (app, api);
            }
            Ok(())
        })
        .build()
}
