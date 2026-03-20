pub mod adapters;
pub mod config;
mod handlers;
mod tokens;

use worker::*;

#[event(fetch)]
async fn main(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    console_error_panic_hook::set_once();

    // Handle CORS preflight
    if req.method() == Method::Options {
        return cors_preflight(&env);
    }

    let router = Router::new();

    let result = router
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
        .run(req, env.clone())
        .await?;

    // Add CORS headers to every response
    add_cors_headers(result, &env)
}

fn cors_preflight(env: &Env) -> Result<Response> {
    let origins = config::cors_origins(env);
    let allow_origin = origins.first().cloned().unwrap_or_else(|| "*".to_string());

    let headers = Headers::new();
    headers.set("Access-Control-Allow-Origin", &allow_origin)?;
    headers.set(
        "Access-Control-Allow-Methods",
        "GET, POST, PUT, PATCH, DELETE, OPTIONS",
    )?;
    headers.set(
        "Access-Control-Allow-Headers",
        "Authorization, Content-Type, Cache-Control, Pragma, Cookie, X-Audience",
    )?;
    headers.set("Access-Control-Allow-Credentials", "true")?;
    headers.set("Access-Control-Max-Age", "86400")?;

    Ok(Response::empty()?.with_status(204).with_headers(headers))
}

fn add_cors_headers(mut response: Response, env: &Env) -> Result<Response> {
    let origins = config::cors_origins(env);
    let allow_origin = origins.first().cloned().unwrap_or_else(|| "*".to_string());

    let headers = response.headers_mut();
    headers.set("Access-Control-Allow-Origin", &allow_origin)?;
    headers.set("Access-Control-Allow-Credentials", "true")?;

    Ok(response)
}
