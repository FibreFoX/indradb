/// Defines a unit test function.
#[macro_export]
macro_rules! define_test {
	($name:ident, $datastore_constructor:expr) => (
		#[test]
		fn $name() {
			let datastore = $datastore_constructor;
			let mut sandbox = ::tests::DatastoreTestSandbox::new(datastore);
			sandbox.setup(stringify!($name));
			::tests::$name(&mut sandbox);
			sandbox.teardown();
		}
	)
}

/// Datastore test suites can use this macro to enable the standard test suite
/// for account management.
#[macro_export]
macro_rules! test_account_management_impl {
	($name:ident $code:expr) => (
		define_test!(should_fail_auth_with_a_bad_username, $code);
		define_test!(should_fail_auth_with_a_bad_password, $code);
		define_test!(should_successfully_auth_with_good_credentials, $code);
		define_test!(should_lookup_valid_accounts, $code);
		define_test!(should_fail_to_lookup_invalid_accounts, $code);
		define_test!(should_fail_when_attempting_to_delete_invalid_accounts, $code);
	)
}

/// Datastore test suites can use this macro to enable the standard test suite
/// for vertex queries.
#[macro_export]
macro_rules! test_vertex_query_impl {
	($name:ident $code:expr) => (
		define_test!(should_get_all_vertices, $code);
		define_test!(should_get_all_vertices_with_zero_limit, $code);
		define_test!(should_get_all_vertices_out_of_range, $code);
		define_test!(should_get_single_vertices, $code);
		define_test!(should_get_single_vertices_nonexisting, $code);
		define_test!(should_get_vertices, $code);
		define_test!(should_get_vertices_piped, $code);
	)
}

/// Datastore test suites can use this macro to enable the standard test suite
/// for transactions.
#[macro_export]
macro_rules! test_transaction_impl {
	($name:ident $code:expr) => (
		define_test!(should_get_a_vertex_range, $code);
		define_test!(should_get_a_valid_vertex, $code);
		define_test!(should_not_get_an_invalid_vertex, $code);
		define_test!(should_update_a_valid_vertex, $code);
		define_test!(should_not_update_an_invalid_vertex, $code);
		define_test!(should_delete_a_valid_vertex, $code);
		define_test!(should_not_delete_an_invalid_vertex, $code);
		define_test!(should_not_delete_an_unowned_vertex, $code);

		define_test!(should_get_a_valid_edge, $code);
		define_test!(should_not_get_an_invalid_edge, $code);
		define_test!(should_update_a_valid_edge, $code);
		define_test!(should_not_update_an_invalid_edge, $code);
		define_test!(should_not_set_an_edge_with_bad_permissions, $code);
		define_test!(should_delete_a_valid_edge, $code);
		define_test!(should_not_delete_an_invalid_edge, $code);
		define_test!(should_not_delete_an_edge_with_bad_permissions, $code);

		define_test!(should_get_an_edge_count, $code);
		define_test!(should_get_an_edge_count_with_no_type, $code);
		define_test!(should_get_an_edge_count_for_an_invalid_edge, $code);
		define_test!(should_get_an_edge_range, $code);
		define_test!(should_get_edge_range_with_no_type, $code);
		define_test!(should_get_no_edges_for_an_invalid_range, $code);
		define_test!(should_get_edge_range_with_no_high, $code);
		define_test!(should_get_edge_range_with_no_low, $code);
		define_test!(should_get_edge_range_with_no_time, $code);
		define_test!(should_get_no_edges_for_reversed_time, $code);

		define_test!(should_get_a_reversed_edge_count, $code);
		define_test!(should_get_a_reversed_edge_count_with_no_type, $code);
		define_test!(should_get_a_reversed_edge_count_for_an_invalid_edge, $code);
		define_test!(should_get_a_reversed_edge_range, $code);
		define_test!(should_get_a_reversed_edge_range_with_no_type, $code);
		define_test!(should_get_no_reversed_edges_for_an_invalid_range, $code);
		define_test!(should_get_a_reversed_edge_range_with_no_high, $code);
		define_test!(should_get_a_reversed_edge_range_with_no_low, $code);
		define_test!(should_get_a_reversed_edge_range_with_no_time, $code);
		define_test!(should_get_no_reversed_edges_for_reversed_time, $code);
	)
}

/// Datastore test suites can use this macro to enable the standard test suite
/// for transaction metadata.
#[macro_export]
macro_rules! test_metadata_impl {
	($name:ident $code:expr) => (
		define_test!(should_handle_global_metadata, $code);
		define_test!(should_handle_account_metadata, $code);
		define_test!(should_not_set_invalid_account_metadata, $code);
		define_test!(should_not_delete_invalid_account_metadata, $code);
		define_test!(should_handle_vertex_metadata, $code);
		define_test!(should_not_set_invalid_vertex_metadata, $code);
		define_test!(should_not_delete_invalid_vertex_metadata, $code);
		define_test!(should_handle_edge_metadata, $code);
		define_test!(should_not_set_invalid_edge_metadata, $code);
		define_test!(should_not_delete_invalid_edge_metadata, $code);
	)
}
