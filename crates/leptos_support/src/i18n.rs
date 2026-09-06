use leptos::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locale {
    En,
    Tr,
}

impl Locale {
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Tr => "tr",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::En => "EN",
            Self::Tr => "TR",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "en" => Some(Self::En),
            "tr" => Some(Self::Tr),
            _ => None,
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            Self::En => Self::Tr,
            Self::Tr => Self::En,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LocaleContext {
    locale: RwSignal<Locale>,
    storage_key: &'static str,
}

impl LocaleContext {
    pub fn new(storage_key: &'static str) -> Self {
        Self {
            locale: RwSignal::new(stored_locale(storage_key)),
            storage_key,
        }
    }

    pub fn locale(self) -> Locale {
        self.locale.get()
    }

    pub fn locale_untracked(self) -> Locale {
        self.locale.get_untracked()
    }

    pub fn set_locale(self, locale: Locale) {
        self.locale.set(locale);
        let _ = self.storage_key;

        #[cfg(feature = "hydrate")]
        if let Some(window) = web_sys::window()
            && let Ok(Some(storage)) = window.local_storage()
        {
            let _ = storage.set_item(self.storage_key, locale.code());
        }
    }

    pub fn toggle(self) {
        self.set_locale(self.locale_untracked().toggled());
    }
}

fn stored_locale(storage_key: &str) -> Locale {
    #[cfg(feature = "hydrate")]
    if let Some(window) = web_sys::window()
        && let Ok(Some(storage)) = window.local_storage()
        && let Ok(Some(value)) = storage.get_item(storage_key)
        && let Some(locale) = Locale::from_code(&value)
    {
        return locale;
    }

    let _ = storage_key;
    Locale::En
}
