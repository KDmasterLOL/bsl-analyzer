use std::sync::Arc;

use base_db::FileIdInput;

use crate::body::{lower_method_with_externals, Body, BodySourceMap};
use crate::{DefDatabase, MethodIdInput};

#[salsa::tracked(lru = 4096, heap_size = crate::body::body_arc_heap)]
pub fn method_body_query<'db>(db: &'db dyn DefDatabase, method: MethodIdInput<'db>) -> Arc<Body> {
    let mid = method.method_id(db);
    let file_id = mid.module.file_id;

    let _span =
        tracing::info_span!("method_body_query", file_id = file_id.0, local_id = mid.local_id)
            .entered();

    let file_id_input = FileIdInput::new(db, file_id);
    let symbol_tree = crate::symbol_tree::symbol_tree_query(db, file_id_input);
    let Some(method_symbol) = symbol_tree.find_method_by_id(mid) else {
        tracing::warn!(?mid, "method_body_query: MethodId not found in symbol tree");
        return Arc::new(Body::default());
    };

    let parse = db.parse(file_id);
    let Some(method_node) = method_symbol.syntax_node(&parse) else {
        tracing::warn!(?mid, "method_body_query: syntax node not found at recorded range");
        return Arc::new(Body::default());
    };

    let result = lower_method_with_externals(&method_node, method_symbol.is_function, None);
    Arc::new(result.body)
}

#[salsa::tracked(lru = 4096, heap_size = crate::body::body_with_source_map_heap)]
pub fn method_body_with_source_map_query<'db>(
    db: &'db dyn DefDatabase,
    method: MethodIdInput<'db>,
) -> Arc<(Body, BodySourceMap)> {
    let mid = method.method_id(db);
    let file_id = mid.module.file_id;

    let _span = tracing::info_span!(
        "method_body_with_source_map_query",
        file_id = file_id.0,
        local_id = mid.local_id,
    )
    .entered();

    let file_id_input = FileIdInput::new(db, file_id);
    let symbol_tree = crate::symbol_tree::symbol_tree_query(db, file_id_input);
    let Some(method_symbol) = symbol_tree.find_method_by_id(mid) else {
        tracing::warn!(
            ?mid,
            "method_body_with_source_map_query: MethodId not found in symbol tree"
        );
        return Arc::new((Body::default(), BodySourceMap::default()));
    };

    let parse = db.parse(file_id);
    let Some(method_node) = method_symbol.syntax_node(&parse) else {
        tracing::warn!(
            ?mid,
            "method_body_with_source_map_query: syntax node not found at recorded range"
        );
        return Arc::new((Body::default(), BodySourceMap::default()));
    };

    let result = lower_method_with_externals(&method_node, method_symbol.is_function, None);
    Arc::new((result.body, result.source_map))
}
