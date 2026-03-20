pub mod adapters;
mod handlers;

use worker::*;

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    let router = Router::new();

    router
        // Health
        .get("/", |_, _| Response::ok("Diaryx Cloudflare Worker"))
        .get("/health", |_, _| Response::ok("OK"))
        // Namespaces
        .post_async("/namespaces", handlers::create_namespace)
        .get_async("/namespaces", handlers::list_namespaces)
        .get_async("/namespaces/:id", handlers::get_namespace)
        .delete_async("/namespaces/:id", handlers::delete_namespace)
        // Objects
        .put_async("/namespaces/:ns_id/objects/*key", handlers::put_object)
        .get_async("/namespaces/:ns_id/objects/*key", handlers::get_object)
        .delete_async("/namespaces/:ns_id/objects/*key", handlers::delete_object)
        // Public objects
        .get_async("/public/:ns_id/objects/*key", handlers::get_public_object)
        // Audiences
        .put_async("/namespaces/:ns_id/audiences/:name", handlers::set_audience)
        .get_async("/namespaces/:ns_id/audiences", handlers::list_audiences)
        .delete_async(
            "/namespaces/:ns_id/audiences/:name",
            handlers::delete_audience,
        )
        // Sessions
        .post_async("/sessions", handlers::create_session)
        .get_async("/sessions/:code", handlers::get_session)
        .delete_async("/sessions/:code", handlers::delete_session)
        // Auth
        .get_async("/auth/me", handlers::get_current_user)
        .post_async("/auth/logout", handlers::logout)
        .post_async("/auth/magic-link", handlers::request_magic_link)
        .get_async("/auth/verify", handlers::verify_magic_link)
        .post_async("/auth/verify-code", handlers::verify_code)
        // Usage
        .get_async("/usage", handlers::get_usage)
        // Run
        .run(req, env)
        .await
}
