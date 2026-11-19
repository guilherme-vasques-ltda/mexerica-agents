use super::*;

#[tokio::test]
async fn codigo_de_grupo_chega_no_canal_pareado() {
    let hub = temp_hub();
    let key = paired_phone(&hub).await;
    let mut phone = Phone::direct(&hub, key);
    assert!(matches!(phone.hello(&hub, false).await, Out::Msg(v) if v["t"] == "welcome"));
    assert!(phone.call(10, "botGroupsEdit", json!({"op":"add","name":"Pesquisa"})).await.get("ok").is_some());
    let response = phone.call(11, "botGroupsEdit", json!({"op":"add","name":"PÉSQUISA"})).await;
    assert_eq!(response["err"]["code"], "name_taken");
    assert!(response["err"]["message"].is_string());
}
