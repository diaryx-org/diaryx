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
