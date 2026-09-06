use leptos::prelude::*;

use crate::shared::i18n::T;

#[derive(Clone, Copy)]
pub struct IdentityRouteLayout {
    render: fn(T, ChildrenFn) -> AnyView,
}

impl IdentityRouteLayout {
    pub const fn new(render: fn(T, ChildrenFn) -> AnyView) -> Self {
        Self { render }
    }

    fn render(self, title: T, children: ChildrenFn) -> AnyView {
        (self.render)(title, children)
    }
}

#[component]
pub fn WorkspaceRouteLayout(title: T, children: ChildrenFn) -> impl IntoView {
    match use_context::<IdentityRouteLayout>() {
        Some(layout) => layout.render(title, children),
        None => view! {
            <main class="workspace-content">
                <div class="route-fade">{children()}</div>
            </main>
        }
        .into_any(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_layout_is_an_explicit_host_contribution() {
        fn render(_: T, children: ChildrenFn) -> AnyView {
            children()
        }

        let _ = IdentityRouteLayout::new(render);
    }
}
