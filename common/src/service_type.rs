use xutil::ServiceType;

pub const AUTH: ServiceType = ServiceType::new(1);
pub const LOGIC: ServiceType = ServiceType::new(2);
pub const GATE: ServiceType = ServiceType::new(3);
pub const PUBLIC: ServiceType = ServiceType::new(4);
pub const QUERY: ServiceType = ServiceType::new(5);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_types_keep_the_application_wire_values() {
        assert_eq!(AUTH.as_i32(), 1);
        assert_eq!(LOGIC.as_i32(), 2);
        assert_eq!(GATE.as_i32(), 3);
        assert_eq!(PUBLIC.as_i32(), 4);
        assert_eq!(QUERY.as_i32(), 5);
    }
}
