pub mod expr;
pub mod external;
pub mod scope;
pub mod types;

pub use expr::{evaluate, resolve_attr};
pub use external::{
    externalizable_root_names, externalizable_root_params, filter_external_params,
    is_direct_param_value,
};
pub use scope::ParamScope;
pub use types::Value;
