//! Portable billing use cases for Stripe/Apple tier management.

use crate::domain::UserTier;
use crate::ports::{BillingStore, ServerCoreError, UserStore};
use tracing::{error, info, warn};

/// Handles tier changes triggered by billing events.
pub struct BillingService<'a> {
    billing_store: &'a dyn BillingStore,
    user_store: &'a dyn UserStore,
}

impl<'a> BillingService<'a> {
    pub fn new(billing_store: &'a dyn BillingStore, user_store: &'a dyn UserStore) -> Self {
        Self {
            billing_store,
            user_store,
        }
    }

    // ========================================================================
    // Stripe
    // ========================================================================

    /// Resolve a Stripe customer ID to a user ID, or return None.
    pub async fn resolve_stripe_user(
        &self,
        customer_id: &str,
    ) -> Result<Option<String>, ServerCoreError> {
        self.billing_store
            .get_user_id_by_stripe_customer_id(customer_id)
            .await
    }

    /// Handle a successful Stripe checkout — save subscription ID and upgrade to Plus.
    pub async fn handle_checkout_completed(
        &self,
        customer_id: &str,
        subscription_id: Option<&str>,
    ) -> Result<(), ServerCoreError> {
        let user_id = match self.resolve_stripe_user(customer_id).await? {
            Some(id) => id,
            None => {
                warn!(
                    "checkout.session.completed for unknown customer: {}",
                    customer_id
                );
                return Ok(());
            }
        };

        if let Some(sub_id) = subscription_id {
            if let Err(e) = self
                .billing_store
                .set_stripe_subscription_id(&user_id, Some(sub_id))
                .await
            {
                error!("Failed to save subscription ID: {}", e);
            }
        }

        self.user_store
            .set_user_tier(&user_id, UserTier::Plus)
            .await?;
        info!("User {} upgraded to Plus via checkout", user_id);
        Ok(())
    }

    /// Handle a Stripe subscription status change.
    pub async fn handle_subscription_updated(
        &self,
        customer_id: &str,
        status: &str,
    ) -> Result<(), ServerCoreError> {
        let user_id = match self.resolve_stripe_user(customer_id).await? {
            Some(id) => id,
            None => {
                warn!(
                    "customer.subscription.updated for unknown customer: {}",
                    customer_id
                );
                return Ok(());
            }
        };

        let tier = match status {
            "active" | "trialing" => UserTier::Plus,
            _ => UserTier::Free,
        };

        self.user_store.set_user_tier(&user_id, tier).await?;
        info!(
            "User {} tier set to {} (subscription status: {})",
            user_id,
            tier.as_str(),
            status
        );
        Ok(())
    }

    /// Handle a Stripe subscription deletion — downgrade to Free.
    pub async fn handle_subscription_deleted(
        &self,
        customer_id: &str,
    ) -> Result<(), ServerCoreError> {
        let user_id = match self.resolve_stripe_user(customer_id).await? {
            Some(id) => id,
            None => {
                warn!(
                    "customer.subscription.deleted for unknown customer: {}",
                    customer_id
                );
                return Ok(());
            }
        };

        self.user_store
            .set_user_tier(&user_id, UserTier::Free)
            .await?;
        info!("User {} downgraded to Free (subscription deleted)", user_id);

        if let Err(e) = self
            .billing_store
            .set_stripe_subscription_id(&user_id, None)
            .await
        {
            error!(
                "Failed to clear subscription ID for user {}: {}",
                user_id, e
            );
        }
        Ok(())
    }

    // ========================================================================
    // Apple IAP
    // ========================================================================

    /// Verify and activate an Apple IAP transaction for a user.
    pub async fn activate_apple_transaction(
        &self,
        user_id: &str,
        original_transaction_id: &str,
    ) -> Result<(), ServerCoreError> {
        self.billing_store
            .set_apple_original_transaction_id(user_id, original_transaction_id)
            .await?;
        self.user_store
            .set_user_tier(user_id, UserTier::Plus)
            .await?;
        info!(
            "User {} upgraded to Plus via Apple IAP (tx: {})",
            user_id, original_transaction_id
        );
        Ok(())
    }

    /// Get the Stripe customer ID for a user.
    pub async fn get_stripe_customer_id(
        &self,
        user_id: &str,
    ) -> Result<Option<String>, ServerCoreError> {
        self.billing_store.get_stripe_customer_id(user_id).await
    }

    /// Store the Stripe customer ID for a user.
    pub async fn set_stripe_customer_id(
        &self,
        user_id: &str,
        customer_id: &str,
    ) -> Result<(), ServerCoreError> {
        self.billing_store
            .set_stripe_customer_id(user_id, customer_id)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::BillingService;
    use crate::domain::UserTier;
    use crate::ports::{BillingStore, ServerCoreError, UserStore};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct TestBillingStore {
        stripe_customer_ids: Mutex<HashMap<String, String>>,
        user_ids_by_customer: Mutex<HashMap<String, String>>,
        stripe_subscription_ids: Mutex<HashMap<String, Option<String>>>,
        apple_transaction_ids: Mutex<HashMap<String, String>>,
        user_ids_by_apple_transaction: Mutex<HashMap<String, String>>,
        fail_subscription_updates: Mutex<bool>,
    }

    crate::cfg_async_trait! {
    impl BillingStore for TestBillingStore {
        async fn get_stripe_customer_id(
            &self,
            user_id: &str,
        ) -> Result<Option<String>, ServerCoreError> {
            Ok(self
                .stripe_customer_ids
                .lock()
                .unwrap()
                .get(user_id)
                .cloned())
        }

        async fn set_stripe_customer_id(
            &self,
            user_id: &str,
            customer_id: &str,
        ) -> Result<(), ServerCoreError> {
            self.stripe_customer_ids
                .lock()
                .unwrap()
                .insert(user_id.to_string(), customer_id.to_string());
            self.user_ids_by_customer
                .lock()
                .unwrap()
                .insert(customer_id.to_string(), user_id.to_string());
            Ok(())
        }

        async fn get_user_id_by_stripe_customer_id(
            &self,
            customer_id: &str,
        ) -> Result<Option<String>, ServerCoreError> {
            Ok(self
                .user_ids_by_customer
                .lock()
                .unwrap()
                .get(customer_id)
                .cloned())
        }

        async fn set_stripe_subscription_id(
            &self,
            user_id: &str,
            subscription_id: Option<&str>,
        ) -> Result<(), ServerCoreError> {
            if *self.fail_subscription_updates.lock().unwrap() {
                return Err(ServerCoreError::internal("failed to persist subscription"));
            }
            self.stripe_subscription_ids.lock().unwrap().insert(
                user_id.to_string(),
                subscription_id.map(str::to_string),
            );
            Ok(())
        }

        async fn get_apple_original_transaction_id(
            &self,
            user_id: &str,
        ) -> Result<Option<String>, ServerCoreError> {
            Ok(self
                .apple_transaction_ids
                .lock()
                .unwrap()
                .get(user_id)
                .cloned())
        }

        async fn set_apple_original_transaction_id(
            &self,
            user_id: &str,
            transaction_id: &str,
        ) -> Result<(), ServerCoreError> {
            self.apple_transaction_ids
                .lock()
                .unwrap()
                .insert(user_id.to_string(), transaction_id.to_string());
            self.user_ids_by_apple_transaction
                .lock()
                .unwrap()
                .insert(transaction_id.to_string(), user_id.to_string());
            Ok(())
        }

        async fn get_user_id_by_apple_transaction_id(
            &self,
            transaction_id: &str,
        ) -> Result<Option<String>, ServerCoreError> {
            Ok(self
                .user_ids_by_apple_transaction
                .lock()
                .unwrap()
                .get(transaction_id)
                .cloned())
        }
    }
    }

    #[derive(Default)]
    struct TestUserStore {
        tiers: Mutex<HashMap<String, UserTier>>,
    }

    crate::cfg_async_trait! {
    impl UserStore for TestUserStore {
        async fn get_or_create_user(&self, email: &str) -> Result<String, ServerCoreError> {
            Ok(format!("user-for-{email}"))
        }

        async fn update_last_login(&self, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }

        async fn delete_user(&self, _: &str) -> Result<(), ServerCoreError> {
            Ok(())
        }

        async fn get_effective_device_limit(&self, _: &str) -> Result<u32, ServerCoreError> {
            Ok(2)
        }

        async fn set_user_tier(
            &self,
            user_id: &str,
            tier: UserTier,
        ) -> Result<(), ServerCoreError> {
            self.tiers.lock().unwrap().insert(user_id.to_string(), tier);
            Ok(())
        }
    }
    }

    fn seed_customer(store: &TestBillingStore, customer_id: &str, user_id: &str) {
        store
            .stripe_customer_ids
            .lock()
            .unwrap()
            .insert(user_id.to_string(), customer_id.to_string());
        store
            .user_ids_by_customer
            .lock()
            .unwrap()
            .insert(customer_id.to_string(), user_id.to_string());
    }

    #[tokio::test]
    async fn checkout_completed_upgrades_user_and_persists_subscription() {
        let billing_store = TestBillingStore::default();
        seed_customer(&billing_store, "cus_123", "user1");
        let user_store = TestUserStore::default();
        let service = BillingService::new(&billing_store, &user_store);

        service
            .handle_checkout_completed("cus_123", Some("sub_123"))
            .await
            .unwrap();

        assert_eq!(
            user_store.tiers.lock().unwrap().get("user1"),
            Some(&UserTier::Plus)
        );
        assert_eq!(
            billing_store
                .stripe_subscription_ids
                .lock()
                .unwrap()
                .get("user1")
                .cloned(),
            Some(Some("sub_123".to_string()))
        );
    }

    #[tokio::test]
    async fn checkout_completed_ignores_unknown_customer() {
        let billing_store = TestBillingStore::default();
        let user_store = TestUserStore::default();
        let service = BillingService::new(&billing_store, &user_store);

        service
            .handle_checkout_completed("unknown", Some("sub_123"))
            .await
            .unwrap();

        assert!(user_store.tiers.lock().unwrap().is_empty());
        assert!(
            billing_store
                .stripe_subscription_ids
                .lock()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn checkout_completed_still_upgrades_when_subscription_persist_fails() {
        let billing_store = TestBillingStore::default();
        seed_customer(&billing_store, "cus_123", "user1");
        *billing_store.fail_subscription_updates.lock().unwrap() = true;
        let user_store = TestUserStore::default();
        let service = BillingService::new(&billing_store, &user_store);

        service
            .handle_checkout_completed("cus_123", Some("sub_123"))
            .await
            .unwrap();

        assert_eq!(
            user_store.tiers.lock().unwrap().get("user1"),
            Some(&UserTier::Plus)
        );
        assert!(
            billing_store
                .stripe_subscription_ids
                .lock()
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn subscription_updated_maps_statuses_to_expected_tiers() {
        let billing_store = TestBillingStore::default();
        seed_customer(&billing_store, "cus_123", "user1");
        let user_store = TestUserStore::default();
        let service = BillingService::new(&billing_store, &user_store);

        service
            .handle_subscription_updated("cus_123", "active")
            .await
            .unwrap();
        assert_eq!(
            user_store.tiers.lock().unwrap().get("user1"),
            Some(&UserTier::Plus)
        );

        service
            .handle_subscription_updated("cus_123", "past_due")
            .await
            .unwrap();
        assert_eq!(
            user_store.tiers.lock().unwrap().get("user1"),
            Some(&UserTier::Free)
        );
    }

    #[tokio::test]
    async fn subscription_deleted_downgrades_user_and_clears_subscription() {
        let billing_store = TestBillingStore::default();
        seed_customer(&billing_store, "cus_123", "user1");
        billing_store
            .stripe_subscription_ids
            .lock()
            .unwrap()
            .insert("user1".to_string(), Some("sub_123".to_string()));
        let user_store = TestUserStore::default();
        let service = BillingService::new(&billing_store, &user_store);

        service
            .handle_subscription_deleted("cus_123")
            .await
            .unwrap();

        assert_eq!(
            user_store.tiers.lock().unwrap().get("user1"),
            Some(&UserTier::Free)
        );
        assert_eq!(
            billing_store
                .stripe_subscription_ids
                .lock()
                .unwrap()
                .get("user1")
                .cloned(),
            Some(None)
        );
    }

    #[tokio::test]
    async fn activate_apple_transaction_upgrades_user_and_records_transaction() {
        let billing_store = TestBillingStore::default();
        let user_store = TestUserStore::default();
        let service = BillingService::new(&billing_store, &user_store);

        service
            .activate_apple_transaction("user1", "apple_tx_123")
            .await
            .unwrap();

        assert_eq!(
            user_store.tiers.lock().unwrap().get("user1"),
            Some(&UserTier::Plus)
        );
        assert_eq!(
            billing_store
                .apple_transaction_ids
                .lock()
                .unwrap()
                .get("user1"),
            Some(&"apple_tx_123".to_string())
        );
    }

    #[tokio::test]
    async fn stripe_customer_id_accessors_delegate_to_store() {
        let billing_store = TestBillingStore::default();
        let user_store = TestUserStore::default();
        let service = BillingService::new(&billing_store, &user_store);

        service
            .set_stripe_customer_id("user1", "cus_123")
            .await
            .unwrap();

        let customer_id = service.get_stripe_customer_id("user1").await.unwrap();
        let resolved_user = service.resolve_stripe_user("cus_123").await.unwrap();

        assert_eq!(customer_id.as_deref(), Some("cus_123"));
        assert_eq!(resolved_user.as_deref(), Some("user1"));
    }
}
