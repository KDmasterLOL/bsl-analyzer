pub mod arg_diagnostics;
pub mod builtin;
pub mod db;
pub mod field_enum;
pub mod field_lookup;
pub mod form_attr;
pub mod form_items;
pub mod form_self;
pub mod infer;
pub mod iteration_lookup;
pub mod lower;
pub mod manager_lookup;
pub mod method_graph;
pub mod method_lookup;
pub mod method_resolution;
pub mod module_implicit;
pub mod narrow;
pub mod object_resolver;
pub mod platform_global_lookup;
pub mod platform_manager_lookup;
pub mod platform_property_lookup;
pub mod platform_resolution;
pub mod proc_signature;
pub mod proc_signature_lookup;
pub mod query_text_dataflow;
pub mod query_unload_refinement;
pub mod sdbl_bridge;
pub mod subtype;
pub mod this_object;
pub mod this_object_attr;

pub use bsl_config::VisibleConfig;
pub use field_enum::{enumerate_fields, FieldInfo, FieldOrigin};
pub use field_lookup::lookup_field;
pub use form_items::{is_form_items_collection_ty, FORM_ITEMS_TYPE_EN, FORM_ITEMS_TYPE_RU};
pub use hir_def::ty::{
    form_control_platform_type_chain, form_control_platform_type_name, form_element_kind_label,
    form_element_kind_sort_band, FormDataKind, FormElementKind, FunctionSignature, MetadataKind,
};
pub use hir_def::type_ref::{BuiltinTypeRef, TypeRef};
pub use hir_def::ConfigsDatabase;
pub use infer::{
    BodyInferenceResult, CallArgBinding, ImplicitLocalAssignment, ImplicitLocalInfo,
    InferOwnerResult, InferenceContext, InferenceDiagnostic, InferenceResult,
    ModuleCodeInferenceResult, ParamsShape, UnresolvedMethodKind,
};
pub use lower::TyLoweringContext;
pub use manager_lookup::{lookup_manager_field, ManagerMemberInfo};
pub use method_lookup::{lookup_method, MethodInfo};
pub use method_resolution::{resolve_qualified_call, MethodResolution};
pub use module_implicit::module_implicit_fields;
pub use object_resolver::{ConfigsObjectResolver, DbObjectResolver, ObjectResolver};
pub use platform_global_lookup::resolve_platform_global_property_type;
pub use platform_manager_lookup::{
    resolve_platform_manager_method, resolve_platform_metadata_ref_method, PlatformMethodResolution,
};
pub use platform_property_lookup::{lookup_platform_property, PlatformPropertyResolution};
pub use platform_resolution::{
    resolve_method, PlatformMethodHandle, PlatformMethodOrigin, ResolvedPlatformMethod,
};
pub use subtype::{is_assignable, is_coercible_to, is_ref_ty};
