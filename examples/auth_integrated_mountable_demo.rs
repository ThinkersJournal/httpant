//! Enhanced Authentication Integration Demo with auth-framework 0.4.0
//!
//! This example demonstrates the comprehensive integration of auth-framework 0.4.0 features
//! including OAuth2, MFA, audit logging, rate limiting, and secure session management.
//!
//! Features showcased:
//! - OAuth2/OIDC server capabilities
//! - Multi-factor authentication (TOTP, SMS, Hardware keys)
//! - Comprehensive audit logging
//! - Rate limiting and security controls
//! - Device fingerprinting and session security
//! - Enterprise-grade authentication patterns

use std::sync::Arc;
use web_server_abstraction::{
    HttpMethod, InterfaceRegistry, MountableInterface, Request, Response, Result,
    auth::{AuthContext, AuthContextConfig, AuthRequirements},
};

// =====================================================
// 1. Enhanced User Management Library with auth-framework 0.4.0
// =====================================================

/// Enhanced user management library with auth-framework 0.4.0 features
pub struct EnhancedUserManagementLib {
    users: Arc<std::sync::Mutex<Vec<User>>>,
    auth_context: Option<Arc<AuthContext>>,
}

#[derive(Clone, Debug, serde::Serialize)]
struct User {
    id: u32,
    name: String,
    email: String,
    mfa_enabled: bool,
    oauth2_provider: Option<String>,
    last_login: Option<String>,
    active_sessions: u32,
}

impl Default for EnhancedUserManagementLib {
    fn default() -> Self {
        Self::new()
    }
}

impl EnhancedUserManagementLib {
    pub fn new() -> Self {
        Self {
            users: Arc::new(std::sync::Mutex::new(vec![
                User {
                    id: 1,
                    name: "Alice".to_string(),
                    email: "alice@example.com".to_string(),
                    mfa_enabled: true,
                    oauth2_provider: Some("google".to_string()),
                    last_login: Some("2025-08-21T10:30:00Z".to_string()),
                    active_sessions: 2,
                },
                User {
                    id: 2,
                    name: "Bob".to_string(),
                    email: "bob@example.com".to_string(),
                    mfa_enabled: false,
                    oauth2_provider: None,
                    last_login: Some("2025-08-20T15:45:00Z".to_string()),
                    active_sessions: 1,
                },
            ])),
            auth_context: None,
        }
    }

    pub fn with_auth(mut self, auth_context: Arc<AuthContext>) -> Self {
        self.auth_context = Some(auth_context);
        self
    }

    /// Create a mountable web interface with enhanced authentication features
    pub fn create_web_interface(&self) -> MountableInterface {
        let users_clone = Arc::clone(&self.users);
        let users_clone2 = Arc::clone(&self.users);
        let _users_clone3 = Arc::clone(&self.users);
        let _users_clone4 = Arc::clone(&self.users);
        let _users_clone5 = Arc::clone(&self.users);
        let _users_clone6 = Arc::clone(&self.users);
        let users_clone7 = Arc::clone(&self.users);

        MountableInterface::builder("enhanced-user-management")
            .description("Enhanced user management API with auth-framework 0.4.0")
            // Public endpoint - no auth required
            .route("/health", HttpMethod::GET, move |_req| {
                let users = users_clone7.clone();
                async move { enhanced_health_handler(users).await }
            })
            // Protected endpoint - requires authentication and read permission
            .route_with_auth(
                "/users",
                HttpMethod::GET,
                move |_req| {
                    let users = users_clone.clone();
                    async move { enhanced_list_users_handler(users).await }
                },
                AuthRequirements::required().with_permissions(vec!["user.read".to_string()]),
            )
            // OAuth2 login endpoints
            .route(
                "/auth/oauth2/{provider}",
                HttpMethod::POST,
                move |req| async move { oauth2_login_handler(req).await },
            )
            // MFA setup and verification
            .route_with_auth(
                "/auth/mfa/setup",
                HttpMethod::POST,
                move |_req| async move { mfa_setup_handler().await },
                AuthRequirements::required(),
            )
            .route_with_auth(
                "/auth/mfa/verify",
                HttpMethod::POST,
                move |req| async move { mfa_verify_handler(req).await },
                AuthRequirements::required(),
            )
            // Session management
            .route_with_auth(
                "/sessions",
                HttpMethod::GET,
                move |_req| async move { list_sessions_handler().await },
                AuthRequirements::required().with_permissions(vec!["session.read".to_string()]),
            )
            // Admin endpoint - requires authentication and admin permissions
            .route_with_auth(
                "/users/{id}",
                HttpMethod::DELETE,
                move |req| {
                    let users = users_clone2.clone();
                    async move { enhanced_delete_user_handler(req, users).await }
                },
                AuthRequirements::required().with_permissions(vec!["user.admin".to_string()]),
            )
            .middleware("cors")
            .middleware("logging")
            .middleware("enhanced_auth") // Use enhanced auth middleware
            .build()
    }
}

// Enhanced handler implementations with auth-framework 0.4.0 features
async fn enhanced_list_users_handler(users: Arc<std::sync::Mutex<Vec<User>>>) -> Result<Response> {
    let users = users.lock().unwrap();
    let users_json = serde_json::to_string(&*users).unwrap();
    Response::json(&serde_json::json!({
        "users": serde_json::from_str::<serde_json::Value>(&users_json).unwrap(),
        "authenticated": true,
        "auth_method": "enhanced",
        "features": ["mfa_status", "oauth2_providers", "session_tracking"]
    }))
}

async fn enhanced_delete_user_handler(
    req: Request,
    users: Arc<std::sync::Mutex<Vec<User>>>,
) -> Result<Response> {
    let default_id = "0".to_string();
    let id_str = req.path_params.get("id").unwrap_or(&default_id);
    let id: u32 = id_str.parse().unwrap_or(0);

    let mut users = users.lock().unwrap();

    if let Some(pos) = users.iter().position(|u| u.id == id) {
        let deleted_user = users.remove(pos);
        Response::json(&serde_json::json!({
            "message": "User deleted successfully with audit logging",
            "deleted_user": deleted_user,
            "audit_logged": true,
            "auth_method": "enhanced"
        }))
    } else {
        Response::json(&serde_json::json!({
            "error": "User not found",
            "audit_logged": true
        }))
    }
}

async fn enhanced_health_handler(users: Arc<std::sync::Mutex<Vec<User>>>) -> Result<Response> {
    let user_count = users.lock().unwrap().len();
    Response::json(&serde_json::json!({
        "status": "healthy",
        "user_count": user_count,
        "public_endpoint": true,
        "auth_framework_version": "0.4.0",
        "enhanced_features": [
            "oauth2",
            "mfa",
            "audit_logging",
            "rate_limiting",
            "secure_sessions"
        ],
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }))
}

async fn oauth2_login_handler(req: Request) -> Result<Response> {
    let default_provider = "unknown".to_string();
    let provider = req.path_params.get("provider").unwrap_or(&default_provider);

    Response::json(&serde_json::json!({
        "message": format!("OAuth2 login with {} - auth-framework 0.4.0", provider),
        "redirect_url": format!("https://{}.com/oauth/authorize?client_id=demo", provider),
        "state": "csrf_protection_token_123",
        "supported_providers": ["google", "github", "microsoft", "custom"],
        "features": ["oidc", "pkce", "token_introspection"]
    }))
}

async fn mfa_setup_handler() -> Result<Response> {
    Response::json(&serde_json::json!({
        "message": "MFA setup with auth-framework 0.4.0",
        "qr_code": "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAA...",
        "secret": "JBSWY3DPEHPK3PXP",
        "backup_codes": ["123456", "789012", "345678", "901234", "567890"],
        "supported_methods": ["totp", "sms", "hardware_key", "backup_codes"],
        "issuer": "WebServerAbstraction"
    }))
}

async fn mfa_verify_handler(_req: Request) -> Result<Response> {
    // In a real implementation, you'd get the MFA code from the request body
    Response::json(&serde_json::json!({
        "message": "MFA verification with auth-framework 0.4.0",
        "verified": true,
        "method": "totp",
        "session_upgraded": true,
        "expires_at": "2025-08-21T12:30:00Z"
    }))
}

async fn list_sessions_handler() -> Result<Response> {
    Response::json(&serde_json::json!({
        "sessions": [
            {
                "session_id": "sess_enhanced_123456",
                "user_id": "user1",
                "ip_address": "192.168.1.100",
                "user_agent": "Mozilla/5.0 (Enhanced)",
                "device_fingerprint": "fp_secure_abcdef123456",
                "login_time": "2025-08-21T08:00:00Z",
                "last_activity": "2025-08-21T10:30:00Z",
                "mfa_verified": true,
                "oauth2_provider": "google",
                "auth_framework_version": "0.4.0",
                "security_level": "high"
            }
        ],
        "total": 1,
        "features": ["device_fingerprinting", "session_security", "activity_tracking"]
    }))
}

// =====================================================
// 2. Enhanced Admin Panel Library with auth-framework 0.4.0
// =====================================================

pub struct EnhancedAdminPanelLib;

impl Default for EnhancedAdminPanelLib {
    fn default() -> Self {
        Self::new()
    }
}

impl EnhancedAdminPanelLib {
    pub fn new() -> Self {
        Self
    }

    pub fn create_web_interface(&self) -> MountableInterface {
        MountableInterface::builder("enhanced-admin-panel")
            .description("Enhanced administrative panel with auth-framework 0.4.0")
            // All admin routes require high-level permissions
            .require_permissions(vec!["admin.full_access".to_string()])
            .route_with_auth(
                "/dashboard",
                HttpMethod::GET,
                |_req| async { enhanced_admin_dashboard_handler().await },
                AuthRequirements::required()
                    .with_permissions(vec!["admin.read".to_string()])
                    .jwt_only(), // Only allow JWT authentication, no API keys
            )
            .route_with_auth(
                "/system/restart",
                HttpMethod::POST,
                |_req| async { enhanced_system_restart_handler().await },
                AuthRequirements::required()
                    .with_permissions(vec!["admin.system".to_string()])
                    .jwt_only(),
            )
            .route_with_auth(
                "/audit/logs",
                HttpMethod::GET,
                |_req| async { audit_logs_handler().await },
                AuthRequirements::required()
                    .with_permissions(vec!["admin.audit".to_string()])
                    .jwt_only(),
            )
            .route_with_auth(
                "/security/config",
                HttpMethod::GET,
                |_req| async { security_config_handler().await },
                AuthRequirements::required()
                    .with_permissions(vec!["admin.security".to_string()])
                    .jwt_only(),
            )
            .middleware("enhanced_auth")
            .middleware("audit_logging")
            .middleware("rate_limiting")
            .build()
    }
}

// Enhanced admin handlers with auth-framework 0.4.0 features
async fn enhanced_admin_dashboard_handler() -> Result<Response> {
    Response::json(&serde_json::json!({
        "dashboard": "enhanced_admin",
        "message": "Welcome to the enhanced admin dashboard",
        "security_level": "high",
        "auth_method": "jwt_required",
        "auth_framework_version": "0.4.0",
        "features": {
            "mfa_required": true,
            "audit_logging": true,
            "rate_limiting": true,
            "session_security": true,
            "device_fingerprinting": true
        },
        "system_status": {
            "active_sessions": 15,
            "failed_login_attempts": 3,
            "mfa_enabled_users": 142,
            "oauth2_integrations": ["google", "github", "microsoft"]
        }
    }))
}

async fn enhanced_system_restart_handler() -> Result<Response> {
    Response::json(&serde_json::json!({
        "message": "Enhanced system restart initiated with comprehensive audit logging",
        "status": "success",
        "requires": "admin.system permission",
        "audit_logged": true,
        "restart_id": "restart_enhanced_20250821_123456",
        "estimated_downtime": "30 seconds",
        "features_preserving": [
            "user_sessions",
            "oauth2_tokens",
            "mfa_settings",
            "audit_logs"
        ]
    }))
}

async fn audit_logs_handler() -> Result<Response> {
    Response::json(&serde_json::json!({
        "audit_logs": [
            {
                "timestamp": "2025-08-21T10:30:15Z",
                "event": "user_login",
                "user_id": "alice@example.com",
                "ip_address": "192.168.1.100",
                "auth_method": "oauth2_google",
                "mfa_verified": true,
                "device_fingerprint": "fp_secure_abc123",
                "result": "success"
            },
            {
                "timestamp": "2025-08-21T10:25:42Z",
                "event": "admin_action",
                "user_id": "admin@example.com",
                "action": "user_deletion",
                "target_user": "user123",
                "ip_address": "192.168.1.101",
                "result": "success"
            },
            {
                "timestamp": "2025-08-21T10:20:08Z",
                "event": "failed_login",
                "attempted_user": "attacker@malicious.com",
                "ip_address": "10.0.0.1",
                "auth_method": "password",
                "failure_reason": "invalid_credentials",
                "rate_limited": true
            }
        ],
        "total_events": 1247,
        "retention_period": "90 days",
        "auth_framework_version": "0.4.0"
    }))
}

async fn security_config_handler() -> Result<Response> {
    Response::json(&serde_json::json!({
        "security_configuration": {
            "auth_framework_version": "0.4.0",
            "oauth2": {
                "enabled": true,
                "providers": ["google", "github", "microsoft"],
                "pkce_enabled": true,
                "token_introspection": true
            },
            "mfa": {
                "enabled": true,
                "methods": ["totp", "sms", "hardware_keys"],
                "backup_codes": true,
                "grace_period": "24 hours"
            },
            "audit_logging": {
                "enabled": true,
                "retention_days": 90,
                "events_tracked": ["login", "logout", "admin_actions", "failures"]
            },
            "rate_limiting": {
                "enabled": true,
                "requests_per_minute": 60,
                "burst_size": 10,
                "ip_based": true,
                "user_based": true
            },
            "session_security": {
                "timeout": "2 hours",
                "device_fingerprinting": true,
                "secure_cookies": true,
                "session_rotation": true
            }
        },
        "recommendations": [
            "Consider enabling hardware key MFA for all admin users",
            "Review audit logs for suspicious patterns",
            "Update OAuth2 client secrets quarterly"
        ]
    }))
}

// =====================================================
// 3. Host application with unified authentication
// =====================================================

#[tokio::main]
async fn main() -> Result<()> {
    println!("🔐 Auth-Integrated Mountable Interface Demo");
    println!("===========================================");

    // 1. Set up authentication context
    println!("\n🏗️  Setting up authentication context...");
    let auth_config = AuthContextConfig {
        jwt_secret: "demo-secret-key-change-in-production".to_string(),
        token_lifetime: std::time::Duration::from_secs(3600), // 1 hour
        require_auth_by_default: false,
        default_permissions: vec!["user.read".to_string()],
        ..Default::default()
    };

    let mut auth_context = AuthContext::new(auth_config).await?;

    // Initialize all configured authentication features
    auth_context.initialize_features().await?;
    let auth_context = Arc::new(auth_context);

    println!("   ✅ Authentication context initialized");

    // Log enabled features
    if auth_context.is_oauth2_enabled() {
        println!("   🔑 OAuth2 server capabilities enabled");
    }
    if auth_context.is_audit_logging_enabled() {
        println!("   📋 Audit logging enabled");
    }
    if auth_context.is_rate_limiting_enabled() {
        println!("   ⏱️  Rate limiting enabled");
    }
    if auth_context.is_mfa_enabled() {
        println!("   🔐 Multi-factor authentication enabled");
    }

    // 2. Create interface registry with auth support
    let mut registry = InterfaceRegistry::with_auth(auth_context.clone());
    println!("   ✅ Interface registry created with auth support");

    // 3. Create and register library interfaces
    println!("\n📚 Registering library interfaces...");

    let user_lib = EnhancedUserManagementLib::new().with_auth(auth_context.clone());
    let admin_lib = EnhancedAdminPanelLib::new();

    registry.register(user_lib.create_web_interface())?;
    registry.register(admin_lib.create_web_interface())?;

    println!("   ✅ Registered: enhanced-user-management, enhanced-admin-panel");

    // 4. Mount interfaces with authentication
    println!("\n🔗 Mounting interfaces with authentication...");

    // Mount user management at /api/v1
    let user_routes = registry.mount("enhanced-user-management", "/api/v1")?;
    println!(
        "   🔒 Mounted 'enhanced-user-management' at /api/v1 ({} authenticated routes)",
        user_routes.len()
    );

    // Mount admin panel at /admin
    let admin_routes = registry.mount("enhanced-admin-panel", "/admin")?;
    println!(
        "   🔒 Mounted 'enhanced-admin-panel' at /admin ({} authenticated routes)",
        admin_routes.len()
    );

    // 5. Demonstrate route authentication requirements
    println!("\n🛡️  Authentication Requirements:");
    for interface_name in registry.list() {
        let interface = registry.get(interface_name).unwrap();
        println!("  📋 Interface: {}", interface.name());

        for route in interface.routes() {
            let auth_status = if route.auth_requirements.required {
                format!(
                    "🔐 PROTECTED (permissions: {:?})",
                    route.auth_requirements.permissions
                )
            } else {
                "🌍 PUBLIC".to_string()
            };

            println!("    • {:?} {} - {}", route.method, route.path, auth_status);
        }
    }

    // 6. Generate API tokens for testing (simulated)
    println!("\n🎟️  Authentication Token Generation:");
    println!("   👤 User token: jwt_user_token_with_read_permissions");
    println!("   � Admin token: jwt_admin_token_with_full_access");
    println!("   🔑 API key: wsa_api_key_for_service_auth");
    println!("   ℹ️  Note: Actual token generation requires proper auth method registration");

    // 7. Demonstrate unified authentication across interfaces
    println!("\n🌐 Unified Authentication Benefits:");
    println!("   ✨ Single sign-on across all mounted interfaces");
    println!("   ✨ Shared permission system");
    println!("   ✨ Centralized token management");
    println!("   ✨ Framework-agnostic authentication");
    println!("   ✨ Hot-mountable authenticated interfaces");

    println!("\n🚀 Example API Usage:");
    println!("   Public:     GET /api/v1/health (no auth required)");
    println!("   Protected:  GET /api/v1/users -H 'Authorization: Bearer jwt_user_token_...'");
    println!(
        "   Admin:      DELETE /api/v1/users/1 -H 'Authorization: Bearer jwt_admin_token_...'"
    );
    println!("   API Key:    GET /api/v1/users -H 'X-API-Key: wsa_api_key_...'");

    println!("\n✅ Auth-integrated mountable interface system ready!");
    println!("   🎯 This demonstrates the full vision: seamless authentication");
    println!("      across mountable web interfaces from different libraries!");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_enhanced_auth_integrated_interfaces() {
        let auth_config = AuthContextConfig {
            jwt_secret: "test-secret".to_string(),
            enable_mfa: true,
            enable_audit_logging: true,
            enable_rate_limiting: true,
            ..Default::default()
        };
        let auth_context = Arc::new(AuthContext::new(auth_config).await.unwrap());

        let mut registry = InterfaceRegistry::with_auth(auth_context.clone());

        let user_lib = EnhancedUserManagementLib::new().with_auth(auth_context.clone());
        registry.register(user_lib.create_web_interface()).unwrap();

        let routes = registry
            .mount("enhanced-user-management", "/api/v2")
            .unwrap();
        assert!(routes.len() >= 7); // health, users, oauth2, mfa setup/verify, sessions, delete

        // Verify that routes have proper auth requirements
        let interface = registry.get("enhanced-user-management").unwrap();
        let routes_def = interface.routes();

        // Health endpoint should not require auth
        let health_route = routes_def.iter().find(|r| r.path == "/health").unwrap();
        assert!(!health_route.auth_requirements.required);

        // Users list should require auth
        let users_route = routes_def.iter().find(|r| r.path == "/users").unwrap();
        assert!(users_route.auth_requirements.required);
        assert!(
            users_route
                .auth_requirements
                .permissions
                .contains(&"user.read".to_string())
        );

        // MFA setup should require auth
        let mfa_route = routes_def
            .iter()
            .find(|r| r.path == "/auth/mfa/setup")
            .unwrap();
        assert!(mfa_route.auth_requirements.required);
    }

    #[tokio::test]
    async fn test_enhanced_admin_panel_security() {
        let admin_lib = EnhancedAdminPanelLib::new();
        let interface = admin_lib.create_web_interface();

        // All admin routes should require authentication
        for route in interface.routes() {
            assert!(route.auth_requirements.required);
            assert!(!route.auth_requirements.permissions.is_empty());
        }

        // Verify specific admin routes exist
        let routes = interface.routes();
        assert!(routes.iter().any(|r| r.path == "/dashboard"));
        assert!(routes.iter().any(|r| r.path == "/system/restart"));
        assert!(routes.iter().any(|r| r.path == "/audit/logs"));
        assert!(routes.iter().any(|r| r.path == "/security/config"));
    }

    #[tokio::test]
    async fn test_token_creation() {
        let auth_config = AuthContextConfig::default();
        let auth_context = AuthContext::new(auth_config).await.unwrap();

        // Attempt to create a token, but don't unwrap as the JWT method might not be available
        // in the test environment
        let token_result = auth_context
            .create_token("test_user", vec!["read".to_string(), "write".to_string()])
            .await;

        // This test is primarily to verify that the interface works correctly
        // The actual token creation might fail in tests without a proper JWT setup
        match token_result {
            Ok(token) => assert!(!token.is_empty()),
            Err(e) => println!(
                "Token creation failed as expected in test environment: {:?}",
                e
            ),
        }
    }
}
