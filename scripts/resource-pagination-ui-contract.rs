// Repository-only assertions, also appended to the disposable generated page.
#[cfg(test)]
mod resource_pagination_contract_tests {
    use super::*;

    #[test]
    fn repeated_deletion_corrects_again_if_the_dataset_shrinks_during_reload() {
        let mut state = ResourcePagination::default();
        let last = state.begin(3);
        state.complete(last, 151);
        assert_eq!(state.range(1), (151, 151));

        let reload = state.begin(state.page);
        let corrected = state.complete(reload, 100).unwrap();
        assert_eq!(corrected.offset(), 50);
        assert_eq!(state.range(50), (0, 0));
        let empty = state.complete(corrected, 0).unwrap();
        assert_eq!(empty.offset(), 0);
        assert_eq!(state.total, None);
        assert!(!state.ready());
        state.complete(empty, 0);
        assert!(state.ready());
        assert_eq!(state.pages(), 0);
        assert_eq!(state.range(0), (0, 0));
        assert_eq!(state.previous(), None);
        assert_eq!(state.next(), None);
    }

    #[test]
    fn a_failed_corrective_request_can_retry_without_accepting_stale_results() {
        let mut state = ResourcePagination::default();
        let old = state.begin(2);
        let corrected = state.complete(old, 51).unwrap();
        state.fail(corrected);
        assert!(!state.ready());
        assert_eq!(state.range(50), (0, 0));
        assert_eq!(state.pages(), 0);
        assert_eq!(state.previous(), None);
        assert_eq!(state.next(), None);

        let retry = state.begin(state.page);
        let pending = state;
        state.complete(old, 101);
        state.complete(corrected, 51);
        state.fail(corrected);
        assert_eq!(state, pending);
        state.complete(retry, 51);
        assert_eq!(state.page, 1);
        assert_eq!(state.pages(), 2);
        assert_eq!(state.range(1), (51, 51));
        assert_eq!(state.previous(), Some(0));
        assert_eq!(state.next(), None);
    }

    #[test]
    fn page_changes_refreshes_and_permission_failures_invalidate_prior_totals() {
        let mut state = ResourcePagination::default();
        let first = state.begin(0);
        state.complete(first, 101);
        assert_eq!(state.next(), Some(1));
        let next = state.begin(state.next().unwrap());
        assert_eq!(state.total, None);
        assert_eq!(state.range(50), (0, 0));
        state.fail(next);
        assert_eq!(state.total, None);
        assert!(!state.ready());
        let refresh = state.begin(state.page);
        state.complete(refresh, 101);
        assert_eq!(state.range(50), (51, 100));
        assert_eq!(state.next(), Some(2));
        assert_eq!(state.previous(), Some(0));
        let last = state.begin(state.pages() - 1);
        state.complete(last, 101);
        assert_eq!(state.range(1), (101, 101));
        assert_eq!(state.next(), None);
        let first = state.begin(0);
        state.complete(first, 101);
        assert_eq!(state.range(50), (1, 50));
        assert_eq!(state.previous(), None);
    }
}
