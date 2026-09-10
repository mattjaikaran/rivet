//! Compile-time plugin contract for Rivet servers.
//!
//! A plugin is a type that implements [`Plugin`], and [`install`] composes one
//! plugin into an [`axum::Router`]. The invariant of this crate: a plugin is
//! resolved at compile time. [`install`] is generic over the plugin type, so
//! the call site monomorphizes, the compiler sees the concrete plugin, and the
//! built binary carries a direct call. Nothing here looks a plugin up at
//! runtime: there is no registry, no trait object, and no `dyn` dispatch, so
//! routing stays visible to the optimizer and to a reader of the call site.
//!
//! ```
//! use axum::Router;
//! use rivet_plugin_api::{Plugin, install};
//!
//! struct Health;
//!
//! impl Plugin for Health {
//!     fn name(&self) -> &'static str {
//!         "health"
//!     }
//!
//!     fn install(self, router: Router) -> Router {
//!         router
//!     }
//! }
//!
//! let _router = install(Router::new(), Health);
//! ```

use axum::Router;

/// One server extension that an application composes at build time.
///
/// Implement this trait on the plugin's own type. `install` takes `self` by
/// value, so a plugin settles its configuration before the server starts and
/// the router it returns needs no per-request plugin lookup. The [`Send`],
/// [`Sync`], and `'static` bounds let the router cross worker threads and
/// outlive the call that composed it.
pub trait Plugin: Send + Sync + 'static {
    /// The plugin's stable name, recorded in the install log.
    ///
    /// The return type is `&'static str`, so the name costs no allocation and
    /// can label log lines and metrics without a copy.
    fn name(&self) -> &'static str;

    /// Merge this plugin's routes and state into `router`.
    ///
    /// The plugin owns its routes. Call the plugin through [`install`] rather
    /// than calling this method directly, so the deployment records the
    /// composition.
    fn install(self, router: Router) -> Router;
}

/// Compose `plugin` into `router`, then log the plugin name at info level.
///
/// This generic function is the composition primitive: `P` is a concrete type
/// at every call site, so the compiler resolves the plugin and inlines the
/// call, with no registry and no trait object in the path. The log line fires
/// after `Plugin::install` returns, so a deployment records what it composed.
pub fn install<P: Plugin>(router: Router, plugin: P) -> Router {
    let name = plugin.name();
    let router = plugin.install(router);
    tracing::info!(plugin = name, "installed plugin");
    router
}
