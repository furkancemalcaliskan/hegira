use icons::{CircleCheck, CircleX, Info, TriangleAlert, X};
use leptos::prelude::*;

use crate::shared::rust_ui::ui::{
    button::{Button, ButtonSize, ButtonVariant},
    sonner::{SonnerContainer, SonnerDirection, SonnerList, SonnerPosition},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
    Info,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppToast {
    id: u64,
    kind: ToastKind,
    title: String,
    description: String,
}

#[derive(Clone, Debug)]
pub struct ToastController {
    items: RwSignal<Vec<AppToast>>,
    next_id: RwSignal<u64>,
}

impl ToastController {
    pub fn new() -> Self {
        Self {
            items: RwSignal::new(Vec::new()),
            next_id: RwSignal::new(1),
        }
    }

    pub fn success(&self, title: impl Into<String>, description: impl Into<String>) {
        self.push(ToastKind::Success, title, description);
    }

    pub fn error(&self, title: impl Into<String>, description: impl Into<String>) {
        self.push(ToastKind::Error, title, description);
    }

    pub fn info(&self, title: impl Into<String>, description: impl Into<String>) {
        self.push(ToastKind::Info, title, description);
    }

    pub fn warning(&self, title: impl Into<String>, description: impl Into<String>) {
        self.push(ToastKind::Warning, title, description);
    }

    pub fn dismiss(&self, id: u64) {
        self.items
            .update(|items| items.retain(|item| item.id != id));
    }

    fn push(&self, kind: ToastKind, title: impl Into<String>, description: impl Into<String>) {
        let id = self.next_id.get_untracked();
        self.next_id.set(id + 1);
        self.items.update(|items| {
            items.push(AppToast {
                id,
                kind,
                title: title.into(),
                description: description.into(),
            });
            if items.len() > 4 {
                items.remove(0);
            }
        });
    }
}

impl Default for ToastController {
    fn default() -> Self {
        Self::new()
    }
}

pub fn use_toast() -> ToastController {
    use_context::<ToastController>().unwrap_or_default()
}

#[component]
pub fn ToastViewport() -> impl IntoView {
    let controller = use_context::<ToastController>().unwrap_or_default();

    view! {
        <SonnerContainer class="right-6 bottom-6".to_string() position=SonnerPosition::BottomRight>
            <SonnerList
                position=SonnerPosition::BottomRight
                direction=SonnerDirection::BottomUp
                class="h-auto w-[min(24rem,calc(100vw-3rem))]".to_string()
            >
                {move || {
                    controller
                        .items
                        .get()
                        .into_iter()
                        .map(|toast| {
                            let id = toast.id;
                            let icon = match toast.kind {
                                ToastKind::Success => view! { <CircleCheck class="size-4 text-success".to_string()/> }.into_any(),
                                ToastKind::Error => view! { <CircleX class="size-4 text-destructive".to_string()/> }.into_any(),
                                ToastKind::Info => view! { <Info class="size-4 text-info".to_string()/> }.into_any(),
                                ToastKind::Warning => view! { <TriangleAlert class="size-4 text-warning".to_string()/> }.into_any(),
                            };

                            let dismiss_controller = controller.clone();
                            view! {
                                <li class="grid grid-cols-[auto_1fr_auto] gap-3 rounded-md border bg-background p-4 text-sm shadow-lg">
                                    <span class="mt-0.5">{icon}</span>
                                    <span class="grid gap-1">
                                        <strong class="font-medium leading-none">{toast.title}</strong>
                                        <span class="text-muted-foreground">{toast.description}</span>
                                    </span>
                                    <Button
                                        variant=ButtonVariant::Ghost
                                        size=ButtonSize::Icon
                                        class="size-7".to_string()
                                        on:click=move |_| dismiss_controller.dismiss(id)
                                    >
                                        <X class="size-3.5".to_string()/>
                                    </Button>
                                </li>
                            }
                        })
                        .collect_view()
                }}
            </SonnerList>
        </SonnerContainer>
    }
}
