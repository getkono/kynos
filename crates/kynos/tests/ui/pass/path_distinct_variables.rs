fn main() {
    let template = kynos::path!("/tenants/{tenantId}/users/{id}");
    assert_eq!(template.variables(), ["tenantId", "id"]);
}
