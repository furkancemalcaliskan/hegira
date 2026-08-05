use leptos::prelude::*;

pub use leptos_support::i18n::Locale;
use leptos_support::i18n::LocaleContext;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum T {
    Account,
    AllRightsReserved,
    Architecture,
    ArchitectureDescription,
    Cancel,
    CloseNavigation,
    ConfirmLogout,
    ConfirmLogoutDescription,
    DddReady,
    Delivery,
    DeliveryDescription,
    Frontend,
    FrontendDescription,
    GoHome,
    GoLogin,
    Home,
    HomeDescription,
    LoggingOut,
    Login,
    Logout,
    LogoutFailed,
    Menu,
    OpenNavigation,
    Page,
    PageNotFound,
    PageNotFoundDescription,
    Profile,
    Roles,
    RustUi,
    Search,
    Settings,
    SingleBinary,
    ToggleCompactSidebar,
    ToggleLanguage,
    ToggleTheme,
    Users,
}

#[derive(Clone, Copy, Debug)]
pub struct I18n {
    locale: LocaleContext,
}

impl I18n {
    pub const fn new(locale: LocaleContext) -> Self {
        Self { locale }
    }

    pub fn locale(self) -> Locale {
        self.locale.locale()
    }

    pub fn toggle_locale(self) {
        self.locale.toggle();
    }

    pub fn t(self, key: T) -> &'static str {
        translate(self.locale.locale(), key)
    }

    pub fn t_untracked(self, key: T) -> &'static str {
        translate(self.locale.locale_untracked(), key)
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new(
            use_context::<LocaleContext>()
                .unwrap_or_else(|| LocaleContext::new("application-locale")),
        )
    }
}

pub fn use_i18n() -> I18n {
    use_context::<I18n>().unwrap_or_default()
}

fn translate(locale: Locale, key: T) -> &'static str {
    match (locale, key) {
        (Locale::En, T::Account) => "Account",
        (Locale::En, T::AllRightsReserved) => "All rights reserved",
        (Locale::En, T::Architecture) => "Architecture",
        (Locale::En, T::ArchitectureDescription) => {
            "Application, domain, infrastructure and presentation boundaries are in place."
        }
        (Locale::En, T::Cancel) => "Cancel",
        (Locale::En, T::CloseNavigation) => "Close navigation",
        (Locale::En, T::ConfirmLogout) => "Sign out?",
        (Locale::En, T::ConfirmLogoutDescription) => {
            "You will need to sign in again to access the workspace."
        }
        (Locale::En, T::DddReady) => "DDD Ready",
        (Locale::En, T::Delivery) => "Delivery",
        (Locale::En, T::DeliveryDescription) => {
            "Leptos SSR/hydration and Axum backend are served from one Rust app."
        }
        (Locale::En, T::Frontend) => "Frontend",
        (Locale::En, T::FrontendDescription) => {
            "Shared UI primitives are sourced from the Rust/UI registry."
        }
        (Locale::En, T::GoHome) => "Go to home",
        (Locale::En, T::GoLogin) => "Go to login",
        (Locale::En, T::Home) => "Home",
        (Locale::En, T::HomeDescription) => "Operational overview for the Leptos + Axum platform.",
        (Locale::En, T::LoggingOut) => "Logging out...",
        (Locale::En, T::Login) => "Login",
        (Locale::En, T::Logout) => "Logout",
        (Locale::En, T::LogoutFailed) => "Logout failed",
        (Locale::En, T::Menu) => "Menu",
        (Locale::En, T::OpenNavigation) => "Open navigation",
        (Locale::En, T::Page) => "Page",
        (Locale::En, T::PageNotFound) => "Page not found",
        (Locale::En, T::PageNotFoundDescription) => "The page you are looking for does not exist.",
        (Locale::En, T::Profile) => "Profile",
        (Locale::En, T::Roles) => "Roles",
        (Locale::En, T::RustUi) => "Rust/UI",
        (Locale::En, T::Search) => "Search",
        (Locale::En, T::Settings) => "Settings",
        (Locale::En, T::SingleBinary) => "Single Binary",
        (Locale::En, T::ToggleCompactSidebar) => "Toggle compact sidebar",
        (Locale::En, T::ToggleLanguage) => "Change language",
        (Locale::En, T::ToggleTheme) => "Toggle theme",
        (Locale::En, T::Users) => "Users",
        (Locale::Tr, T::Account) => "Hesap",
        (Locale::Tr, T::AllRightsReserved) => "Tüm hakları saklıdır",
        (Locale::Tr, T::Architecture) => "Mimari",
        (Locale::Tr, T::ArchitectureDescription) => {
            "Application, domain, infrastructure ve presentation sınırları hazır."
        }
        (Locale::Tr, T::Cancel) => "İptal",
        (Locale::Tr, T::CloseNavigation) => "Navigasyonu kapat",
        (Locale::Tr, T::ConfirmLogout) => "Çıkış yapılsın mı?",
        (Locale::Tr, T::ConfirmLogoutDescription) => {
            "Çalışma alanına erişmek için tekrar giriş yapmanız gerekecek."
        }
        (Locale::Tr, T::DddReady) => "DDD hazır",
        (Locale::Tr, T::Delivery) => "Dağıtım",
        (Locale::Tr, T::DeliveryDescription) => {
            "Leptos SSR/hydration ve Axum backend tek Rust uygulamasından sunulur."
        }
        (Locale::Tr, T::Frontend) => "Frontend",
        (Locale::Tr, T::FrontendDescription) => {
            "Paylaşılan UI primitive'leri Rust/UI registry'den geliyor."
        }
        (Locale::Tr, T::GoHome) => "Ana sayfaya git",
        (Locale::Tr, T::GoLogin) => "Giriş ekranına git",
        (Locale::Tr, T::Home) => "Ana sayfa",
        (Locale::Tr, T::HomeDescription) => "Leptos + Axum platformu için operasyonel özet.",
        (Locale::Tr, T::LoggingOut) => "Çıkış yapılıyor...",
        (Locale::Tr, T::Login) => "Giriş",
        (Locale::Tr, T::Logout) => "Çıkış",
        (Locale::Tr, T::LogoutFailed) => "Çıkış yapılamadı",
        (Locale::Tr, T::Menu) => "Menü",
        (Locale::Tr, T::OpenNavigation) => "Navigasyonu aç",
        (Locale::Tr, T::Page) => "Sayfa",
        (Locale::Tr, T::PageNotFound) => "Sayfa bulunamadı",
        (Locale::Tr, T::PageNotFoundDescription) => "Aradığınız sayfa mevcut değil.",
        (Locale::Tr, T::Profile) => "Profil",
        (Locale::Tr, T::Roles) => "Roller",
        (Locale::Tr, T::RustUi) => "Rust/UI",
        (Locale::Tr, T::Search) => "Ara",
        (Locale::Tr, T::Settings) => "Ayarlar",
        (Locale::Tr, T::SingleBinary) => "Tek binary",
        (Locale::Tr, T::ToggleCompactSidebar) => "Dar kenar çubuğu modunu değiştir",
        (Locale::Tr, T::ToggleLanguage) => "Dili değiştir",
        (Locale::Tr, T::ToggleTheme) => "Temayı değiştir",
        (Locale::Tr, T::Users) => "Kullanıcılar",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_copy_is_owned_without_compatibility_localization() {
        assert_eq!(translate(Locale::En, T::Home), "Home");
        assert_eq!(translate(Locale::Tr, T::Home), "Ana sayfa");
    }
}
