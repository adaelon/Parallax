use eam_core::{
    IdentityProfileSnapshot, IdentityRuntimeContext, IdentityStateSnapshot, InMemoryRepository,
    Timestamp,
};

pub fn ready_repository() -> InMemoryRepository {
    let identity = IdentityStateSnapshot::restore(
        1,
        None,
        IdentityProfileSnapshot::new(
            "测试第二自我",
            "清晰表达",
            "保留独立判断",
            "可追溯性优先",
            "共同回看的同行者",
            "帮助本人形成更准确的自我理解",
        ),
        "确定性测试夹具",
        Vec::new(),
        Timestamp::from_millis(1),
    );
    InMemoryRepository::new()
        .with_identity_context(IdentityRuntimeContext::new(1, 1, identity))
        .expect("the shared test counterpart fixture is internally consistent")
}
