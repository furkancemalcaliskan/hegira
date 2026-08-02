use leptos::prelude::*;

pub use domain_shared::localization::T;
pub use leptos_support::i18n::Locale;
use leptos_support::i18n::LocaleContext;

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
    let locale = match locale {
        Locale::En => domain_shared::localization::Locale::En,
        Locale::Tr => domain_shared::localization::Locale::Tr,
    };
    domain_shared::localization::translate(locale, key)
}
