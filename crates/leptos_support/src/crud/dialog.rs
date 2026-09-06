use icons::X;
use leptos::prelude::*;

use crate::rust_ui::ui::{
    button::{Button, ButtonVariant},
    dialog::{DialogDescription, DialogHeader, DialogTitle},
};

#[component]
pub fn CrudDialog(
    open: RwSignal<bool>,
    title: Signal<String>,
    description: Signal<String>,
    on_close: Callback<()>,
    children: Children,
) -> impl IntoView {
    let backdrop_class = move || {
        if open.get() {
            "modal-overlay is-open fixed inset-0 z-[1000] grid place-items-center bg-black/50 p-4"
        } else {
            "modal-overlay pointer-events-none fixed inset-0 z-[1000] grid place-items-center bg-black/50 p-4"
        }
    };
    let panel_class = move || {
        if open.get() {
            "modal-panel is-open relative grid w-full max-w-xl gap-5 rounded-lg border bg-background p-6 text-foreground shadow-lg"
        } else {
            "modal-panel relative grid w-full max-w-xl gap-5 rounded-lg border bg-background p-6 text-foreground shadow-lg"
        }
    };

    view! {
        <div class=backdrop_class aria-hidden=move || (!open.get()).to_string()>
            <section role="dialog" aria-modal="true" class=panel_class>
                <DialogHeader>
                    <DialogTitle>{move || title.get()}</DialogTitle>
                    <DialogDescription>{move || description.get()}</DialogDescription>
                </DialogHeader>
                <Button
                    variant=ButtonVariant::Ghost
                    class="absolute right-4 top-4 size-8 px-0".to_string()
                    on:click=move |ev| {
                        ev.prevent_default();
                        on_close.run(());
                    }
                >
                    <X class="size-4".to_string()/>
                </Button>
                {children()}
            </section>
        </div>
    }
}
