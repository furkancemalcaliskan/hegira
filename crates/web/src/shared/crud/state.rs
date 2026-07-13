use leptos::prelude::*;

use crate::shared::data::mutation::MutationStatus;

pub struct CrudListState<Item: Send + Sync + 'static, Sort: Copy + Send + Sync + 'static> {
    pub items: RwSignal<Vec<Item>>,
    pub total: RwSignal<i64>,
    pub page: RwSignal<u32>,
    pub search: RwSignal<String>,
    pub sorting: RwSignal<Sort>,
    pub loading: RwSignal<bool>,
    pub error: RwSignal<Option<String>>,
    pub mutation: RwSignal<MutationStatus>,
    page_size: u32,
}

impl<Item, Sort> Copy for CrudListState<Item, Sort>
where
    Item: Send + Sync + 'static,
    Sort: Copy + Send + Sync + 'static,
{
}

impl<Item, Sort> Clone for CrudListState<Item, Sort>
where
    Item: Send + Sync + 'static,
    Sort: Copy + Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<Item, Sort> CrudListState<Item, Sort>
where
    Item: Send + Sync + 'static,
    Sort: Copy + Send + Sync + 'static,
{
    pub fn new(sorting: Sort, page_size: u32) -> Self {
        Self {
            items: RwSignal::new(Vec::new()),
            total: RwSignal::new(0),
            page: RwSignal::new(1),
            search: RwSignal::new(String::new()),
            sorting: RwSignal::new(sorting),
            loading: RwSignal::new(false),
            error: RwSignal::new(None),
            mutation: RwSignal::new(MutationStatus::Idle),
            page_size: page_size.max(1),
        }
    }

    pub fn begin_load(&self) {
        self.loading.set(true);
        self.error.set(None);
    }

    pub fn fail(&self, error: impl Into<String>) {
        let error = error.into();
        self.error.set(Some(error.clone()));
        self.mutation.set(MutationStatus::Failed(error));
        self.loading.set(false);
    }

    pub fn total_pages(&self) -> u32 {
        if self.total.get() <= 0 {
            1
        } else {
            (self.total.get() as u32).div_ceil(self.page_size)
        }
    }

    pub fn page_size(&self) -> u32 {
        self.page_size
    }
}
