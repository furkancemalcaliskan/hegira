use std::{collections::HashMap, sync::OnceLock};

use leptos::prelude::*;
pub use leptos_support::i18n::Locale;
use leptos_support::i18n::LocaleContext;

macro_rules! identity_texts {
    ($($variant:ident),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum T { $($variant),+ }

        impl T {
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => stringify!($variant)),+ }
            }
        }
    };
}

identity_texts!(
    AccountCreated,
    AccountReady,
    Action,
    AddPermissions,
    AddRole,
    AddUser,
    AllRoles,
    AllUsers,
    AlreadyHaveAccount,
    AuthenticationFailed,
    BackToLogin,
    Cancel,
    CompleteAccount,
    ConfirmDisconnect,
    ConfirmDisconnectDescription,
    Connect,
    ConnectedAccounts,
    ConnectedAccountsDescription,
    ContinueWithGithub,
    ContinueWithGoogle,
    CreateAccount,
    Created,
    CreateRole,
    CreateRoleDescription,
    CreateUser,
    CreateUserDescription,
    CreateWorkspaceAccount,
    Creating,
    CredentialsRequired,
    Delete,
    DeleteRecord,
    DeleteRecordDescriptionPrefix,
    DeleteRole,
    DeleteRoleDescriptionPrefix,
    Disconnect,
    Edit,
    EditRole,
    EditRoleDescription,
    EditUser,
    EditUserDescription,
    Filter,
    GoHome,
    GoLogin,
    IdentityUser,
    InvalidCredentials,
    InvalidOAuthCallback,
    Loading,
    Login,
    LoginHeroDescription,
    LoginHeroKicker,
    LoginHeroTitle,
    NeedAccount,
    Next,
    NoRolesFound,
    NoRolesFoundDescription,
    NoUsersFound,
    NoUsersFoundDescription,
    OAuthAuthentication,
    OAuthCancelled,
    OAuthCompleting,
    Or,
    Password,
    PasswordKeepCurrent,
    PasswordRequiredForNewUsers,
    Pending,
    PermissionsSaved,
    PermissionsSaveFailed,
    PermissionStatus,
    Previous,
    Profile,
    ProfileDescription,
    ProtectedAdminCannotBeDeleted,
    Reset,
    Role,
    RoleCreated,
    RoleDeleted,
    RoleDeleteFailed,
    RoleName,
    RoleNameRequired,
    RolePermissions,
    RolePermissionsDescription,
    Roles,
    RoleSaveFailed,
    RolesDescription,
    RolesWithoutPermissions,
    RolesWithPermissions,
    RoleUpdated,
    SavePermissions,
    SaveRole,
    SaveUser,
    Saving,
    Search,
    SignIn,
    SigningIn,
    SignInWorkspace,
    Status,
    TemporaryPassword,
    ToggleLanguage,
    TotpCode,
    TotpRequired,
    Unauthorized,
    UnauthorizedDescription,
    User,
    UserCreated,
    UserCreatedDescription,
    UserDeleted,
    UserDeleteFailed,
    Username,
    UsernameRequired,
    UserRolesDescription,
    Users,
    UserSaveFailed,
    UsersDescription,
    UserUpdated,
    UserUpdatedDescription,
    Verification,
    Verified,
    VerifiedDescription,
    Verify,
    WelcomeBack,
);

#[derive(Clone, Copy, Debug)]
pub struct I18n {
    locale: LocaleContext,
}

impl I18n {
    pub fn new(locale: LocaleContext) -> Self {
        Self { locale }
    }

    pub fn locale(&self) -> Locale {
        self.locale.locale()
    }

    pub fn set_locale(&self, locale: Locale) {
        self.locale.set_locale(locale);
    }

    pub fn toggle_locale(&self) {
        self.locale.toggle();
    }

    pub fn t(&self, key: T) -> &'static str {
        translate(self.locale.locale(), key)
    }

    pub fn t_untracked(&self, key: T) -> &'static str {
        translate(self.locale.locale_untracked(), key)
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new(
            use_context::<LocaleContext>().unwrap_or_else(|| LocaleContext::new("hegira-locale")),
        )
    }
}

pub fn use_i18n() -> I18n {
    use_context::<I18n>().unwrap_or_default()
}

fn translate(locale: Locale, key: T) -> &'static str {
    let key_name = key.as_str();
    resources(locale)
        .get(key_name)
        .map(String::as_str)
        .unwrap_or(key_name)
}

fn resources(locale: Locale) -> &'static HashMap<String, String> {
    static EN: OnceLock<HashMap<String, String>> = OnceLock::new();
    static TR: OnceLock<HashMap<String, String>> = OnceLock::new();

    match locale {
        Locale::En => EN.get_or_init(|| parse_resource(include_str!("i18n/en.json"))),
        Locale::Tr => TR.get_or_init(|| parse_resource(include_str!("i18n/tr.json"))),
    }
}

fn parse_resource(source: &str) -> HashMap<String, String> {
    serde_json::from_str(source).expect("Identity localization resource must be valid JSON")
}
