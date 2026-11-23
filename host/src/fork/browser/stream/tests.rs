use super::*;

struct Owner;
impl Authority for Owner {
    fn is_owner(&self, session: &str, recipient: &str) -> bool {
        session == "s" && recipient == "dono"
    }
}
fn frame(sequence: u64, target: &str, generation: u64) -> Frame {
    Frame {
        session: "s".into(),
        target: target.into(),
        target_generation: generation,
        sequence,
        image: Size { width: 960, height: 540 },
        viewport: Size { width: 1280, height: 720 },
        jpeg: vec![0xff, 0xd8, u8::try_from(sequence).unwrap()],
    }
}
fn input(sequence: u64) -> Input {
    Input {
        session: "s".into(),
        target: "a".into(),
        frame_sequence: 1,
        target_generation: 0,
        sequence,
        epoch: 0,
        controller: Controller::Agent,
        x: 250.0,
        y: 250.0,
        display_width: 500.0,
        display_height: 500.0,
    }
}
#[test]
fn cliente_lento_prioridade_e_desconexao() {
    let mut s = Stream::new("s".into(), "a".into(), true);
    s.enable_test_input_consumer();
    assert!(s.offer(frame(1, "a", 0)));
    assert!(s.offer(frame(2, "a", 0)));
    assert_eq!(s.rates(), (65, 14));
    assert_eq!(s.next_frame(&Owner, "dono").unwrap().sequence, 2);
    let mut i = input(1);
    i.frame_sequence = 2;
    assert!(s.accept_input(&Owner, "dono", &i));
    assert!(s.offer(frame(3, "a", 0)));
    assert!(s.next_frame(&Owner, "dono").is_none());
    assert_eq!(s.next_input(&Owner, "dono").unwrap().sequence, 1);
    assert_eq!(s.next_frame(&Owner, "dono").unwrap().sequence, 3);
    s.disconnect();
    assert!(!s.offer(frame(4, "a", 0)));
    assert!(s.next_frame(&Owner, "dono").is_none());
}
#[test]
fn capacidade_dono_e_revogacao() {
    let mut old = Stream::new("s".into(), "a".into(), false);
    assert!(!old.offer(frame(1, "a", 0)));
    let mut s = Stream::new("s".into(), "a".into(), true);
    s.enable_test_input_consumer();
    assert!(s.offer(frame(1, "a", 0)));
    assert!(s.next_frame(&Owner, "outro").is_none());
    assert!(s.next_frame(&Owner, "dono").is_some());
    assert!(!s.accept_input(&Owner, "outro", &input(1)));
}
#[test]
fn troca_target_e_epoca() {
    let mut s = Stream::new("s".into(), "a".into(), true);
    s.enable_test_input_consumer();
    assert!(s.offer(frame(1, "a", 0)));
    assert!(s.next_frame(&Owner, "dono").is_some());
    assert_eq!(s.set_controller(Controller::Person), Some(1));
    assert!(!s.accept_input(&Owner, "dono", &input(1)));
    let mut i = input(1);
    i.epoch = 1;
    i.controller = Controller::Person;
    assert!(s.accept_input(&Owner, "dono", &i));
    assert_eq!(s.set_target("b".into()), Some(1));
    assert_eq!(s.set_target("a".into()), Some(2));
    assert!(!s.offer(frame(2, "a", 0)));
    assert!(s.offer(frame(1, "a", 2)));
    assert!(s.next_frame(&Owner, "dono").is_some());
}
#[test]
fn coordenadas_com_margens_e_escala() {
    let image = Size { width: 960, height: 540 };
    let viewport = Size { width: 1280, height: 720 };
    assert_eq!(map_point(image, viewport, 500.0, 500.0, 250.0, 250.0), Some((640.0, 360.0)));
    assert_eq!(map_point(image, viewport, 500.0, 500.0, 250.0, 10.0), None);
    assert_eq!(map_point(image, viewport, 1920.0, 1080.0, 960.0, 540.0), Some((640.0, 360.0)));
}
