use leptos::prelude::*;

use domain_shared::localization::translate;
pub use domain_shared::localization::{Locale, T};

#[derive(Clone, Copy, Debug)]
pub struct I18n {
    locale: RwSignal<Locale>,
}

impl I18n {
    pub fn new(locale: Locale) -> Self {
        Self {
            locale: RwSignal::new(locale),
        }
    }

    pub fn locale(&self) -> Locale {
        self.locale.get()
    }

    pub fn set_locale(&self, locale: Locale) {
        self.locale.set(locale);

        #[cfg(feature = "hydrate")]
        {
            if let Some(window) = web_sys::window()
                && let Ok(Some(storage)) = window.local_storage()
            {
                let _ = storage.set_item(Locale::STORAGE_KEY, locale.code());
            }
        }
    }

    pub fn toggle_locale(&self) {
        self.set_locale(self.locale.get_untracked().toggled());
    }

    pub fn t(&self, key: T) -> &'static str {
        translate(self.locale.get(), key)
    }

    pub fn t_untracked(&self, key: T) -> &'static str {
        translate(self.locale.get_untracked(), key)
    }
}

impl Default for I18n {
    fn default() -> Self {
        Self::new(stored_locale())
    }
}

pub fn use_i18n() -> I18n {
    use_context::<I18n>().unwrap_or_default()
}

pub fn stored_locale() -> Locale {
    #[cfg(feature = "hydrate")]
    {
        if let Some(window) = web_sys::window()
            && let Ok(Some(storage)) = window.local_storage()
            && let Ok(Some(value)) = storage.get_item(Locale::STORAGE_KEY)
            && let Some(locale) = Locale::from_code(&value)
        {
            return locale;
        }
    }

    Locale::En
}
