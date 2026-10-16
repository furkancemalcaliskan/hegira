// This presentation-only state is emitted verbatim and tested by the generator.
// Server functions still delegate authorization and queries to the application.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ResourcePagination {
    page: u64,
    total: Option<u64>,
    loading: bool,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResourcePageRequest {
    page: u64,
    generation: u64,
}

impl ResourcePageRequest {
    const LIMIT: u32 = 50;

    fn offset(self) -> u64 {
        self.page * u64::from(Self::LIMIT)
    }
}

impl ResourcePagination {
    fn begin(&mut self, page: u64) -> ResourcePageRequest {
        // Both providers bind offsets as signed 64-bit integers.
        self.page = page.min(i64::MAX as u64 / u64::from(ResourcePageRequest::LIMIT));
        self.generation = self.generation.wrapping_add(1);
        self.total = None;
        self.loading = true;
        ResourcePageRequest {
            page: self.page,
            generation: self.generation,
        }
    }

    fn is_current(self, request: ResourcePageRequest) -> bool {
        self.loading && self.generation == request.generation && self.page == request.page
    }

    fn complete(
        &mut self,
        request: ResourcePageRequest,
        total: u64,
    ) -> Option<ResourcePageRequest> {
        if !self.is_current(request) {
            return None;
        }
        let last = total.saturating_sub(1) / u64::from(ResourcePageRequest::LIMIT);
        if self.page > last {
            // Deletion or refresh can invalidate even several trailing pages.
            // Request the corrected page before publishing rows or counts.
            return Some(self.begin(last));
        }
        self.total = Some(total);
        self.loading = false;
        None
    }

    fn fail(&mut self, request: ResourcePageRequest) {
        if self.is_current(request) {
            self.total = None;
            self.loading = false;
        }
    }

    fn ready(self) -> bool {
        !self.loading && self.total.is_some()
    }

    fn previous(self) -> Option<u64> {
        self.ready().then(|| self.page.checked_sub(1)).flatten()
    }

    fn next(self) -> Option<u64> {
        (self.ready() && self.page + 1 < self.pages()).then_some(self.page + 1)
    }

    fn pages(self) -> u64 {
        self.total
            .filter(|total| *total > 0)
            .map(|total| (total - 1) / u64::from(ResourcePageRequest::LIMIT) + 1)
            .unwrap_or_default()
    }

    fn range(self, visible: usize) -> (u64, u64) {
        match self.total {
            Some(total) if self.ready() && total > 0 && visible > 0 => {
                let offset = self.page * u64::from(ResourcePageRequest::LIMIT);
                (offset + 1, offset.saturating_add(visible as u64).min(total))
            }
            _ => (0, 0),
        }
    }
}

#[cfg(test)]
mod pagination_tests {
    use super::*;

    #[test]
    fn all_records_are_reachable_and_navigation_stops_at_boundaries() {
        for (total, expected_pages) in [(0, 0), (1, 1), (50, 1), (51, 2), (100, 2), (101, 3)] {
            let mut state = ResourcePagination::default();
            let mut visited = 0;
            let mut page = 0;
            loop {
                let request = state.begin(page);
                assert!(!state.ready());
                assert_eq!(state.previous(), None);
                assert_eq!(state.next(), None);
                assert_eq!(state.complete(request, total), None);
                let visible = total.saturating_sub(request.offset()).min(50);
                visited += visible;
                assert_eq!(state.pages(), expected_pages);
                assert_eq!(state.previous(), page.checked_sub(1));
                assert_eq!(
                    state.range(visible as usize),
                    if visible == 0 {
                        (0, 0)
                    } else {
                        (request.offset() + 1, request.offset() + visible)
                    }
                );
                match state.next() {
                    Some(next) => page = next,
                    None => break,
                }
            }
            assert_eq!(visited, total);
        }
    }

    #[test]
    fn deletion_and_refresh_correct_out_of_range_pages_before_success() {
        for (page, total, corrected) in [(2, 100, 1), (9, 51, 1), (1, 0, 0)] {
            let mut state = ResourcePagination::default();
            let request = state.begin(page);
            let retry = state.complete(request, total).unwrap();
            assert_eq!(retry.page, corrected);
            assert!(state.loading);
            assert!(!state.ready());
            assert_eq!(state.total, None);
            assert!(!state.is_current(request));
            assert_eq!(state.complete(retry, total), None);
            assert!(state.ready());
            assert_eq!(state.page, corrected);
        }
    }

    #[test]
    fn stale_success_and_failure_cannot_replace_a_new_request() {
        let mut state = ResourcePagination::default();
        let old = state.begin(0);
        let current = state.begin(1);
        let pending = state;
        assert_eq!(state.complete(old, 1), None);
        state.fail(old);
        assert_eq!(state, pending);
        assert!(state.is_current(current));
        state.complete(current, 51);
        let ready = state;
        state.complete(old, 0);
        state.fail(old);
        assert_eq!(state, ready);
        // A refresh of the same page also invalidates earlier responses.
        let refresh = state.begin(1);
        assert!(!state.is_current(current));
        state.fail(refresh);
        assert!(!state.loading);
        assert!(!state.ready());
        assert_eq!(state.total, None);
        assert_eq!(state.range(50), (0, 0));
        assert_eq!(state.previous(), None);
        assert_eq!(state.next(), None);
        let retry = state.begin(state.page);
        state.complete(retry, 51);
        assert!(state.ready());
    }

    #[test]
    fn extreme_pages_keep_provider_offsets_representable() {
        let mut state = ResourcePagination::default();
        let request = state.begin(u64::MAX);
        assert!(i64::try_from(request.offset()).is_ok());
        assert!(state.complete(request, i64::MAX as u64).is_none());
        assert_eq!(state.next(), None);
        assert_eq!(state.range(7).1, i64::MAX as u64);
    }
}
