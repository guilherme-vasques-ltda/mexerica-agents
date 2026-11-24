//! Núcleo independente do transporte. Ainda não expõe capacidade ao app.
#![allow(dead_code)]

pub(crate) mod canal;
mod codec;
#[cfg(test)]
mod recibo;

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub session: String,
    pub target: String,
    pub sequence: u64,
    pub target_generation: u64,
    pub image: Size,
    pub viewport: Size,
    pub jpeg: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controller {
    Person,
    Agent,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub session: String,
    pub target: String,
    pub frame_sequence: u64,
    pub target_generation: u64,
    pub sequence: u64,
    pub epoch: u64,
    pub controller: Controller,
    /// Posição relativa à caixa inteira onde o quadro foi exibido.
    pub x: f64,
    pub y: f64,
    pub display_width: f64,
    pub display_height: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AcceptedInput {
    pub sequence: u64,
    epoch: u64,
    target_generation: u64,
    #[cfg(test)]
    recibo: uuid::Uuid,
    pub x: f64,
    pub y: f64,
}

/// A implementação concreta deve consultar a autoridade atual a cada envio.
pub trait Authority {
    fn is_owner(&self, session: &str, recipient: &str) -> bool;
}

/// Estado por conexão: não compartilha fila ou confirmação entre destinatários.
pub struct Stream {
    session: String,
    target: String,
    connected: bool,
    capable: bool,
    input_consumer_ready: bool,
    epoch: u64,
    controller: Controller,
    target_generation: u64,
    last_frame: u64,
    displayed: Option<Frame>,
    pending: Option<Frame>,
    last_input: u64,
    inputs: VecDeque<AcceptedInput>,
    quality: u8,
    fps: u8,
}

impl Stream {
    pub fn new(session: String, target: String, capable: bool) -> Self {
        Self {
            session,
            target,
            connected: true,
            capable,
            input_consumer_ready: false,
            epoch: 0,
            controller: Controller::Agent,
            target_generation: 0,
            last_frame: 0,
            displayed: None,
            pending: None,
            last_input: 0,
            inputs: VecDeque::new(),
            quality: 70,
            fps: 15,
        }
    }

    // Só a fixture simula um consumidor nesta etapa. O canal real não tem
    // executor de entrada, mesmo quando negocia quadros.
    #[cfg(test)]
    pub fn enable_test_input_consumer(&mut self) {
        self.input_consumer_ready = true;
    }

    pub fn disconnect(&mut self) {
        self.connected = false;
        self.pending = None;
        self.displayed = None;
        self.inputs.clear();
    }

    pub fn set_target(&mut self, target: String) -> Option<u64> {
        self.target_generation = self.target_generation.checked_add(1)?;
        self.target = target;
        self.last_frame = 0;
        self.pending = None;
        self.displayed = None;
        self.inputs.clear();
        Some(self.target_generation)
    }

    pub fn set_controller(&mut self, controller: Controller) -> Option<u64> {
        self.epoch = self.epoch.checked_add(1)?;
        self.controller = controller;
        self.inputs.clear();
        Some(self.epoch)
    }

    pub fn offer(&mut self, frame: Frame) -> bool {
        if !self.connected
            || !self.capable
            || frame.session != self.session
            || frame.target != self.target
            || frame.target_generation != self.target_generation
            || frame.sequence <= self.last_frame
            || frame.image.width == 0
            || frame.image.height == 0
            || frame.viewport.width == 0
            || frame.viewport.height == 0
            || frame.jpeg.is_empty()
        {
            return false;
        }
        self.last_frame = frame.sequence;
        if self.pending.replace(frame).is_some() {
            self.quality = self.quality.saturating_sub(5).max(30);
            self.fps = self.fps.saturating_sub(1).max(2);
        }
        true
    }

    /// Entrada sempre sai antes da imagem; autoridade é verificada no momento da retirada.
    pub fn next_frame(&mut self, authority: &impl Authority, recipient: &str) -> Option<Frame> {
        if !self.connected || !self.capable || !self.inputs.is_empty() || !authority.is_owner(&self.session, recipient)
        {
            return None;
        }
        let frame = self.pending.take()?;
        self.displayed = Some(frame.clone());
        Some(frame)
    }

    #[cfg(test)]
    pub fn accept_input(&mut self, authority: &impl Authority, recipient: &str, input: &Input) -> bool {
        authority.is_owner(&self.session, recipient) && self.accept_authorized_input(input)
    }

    pub(crate) fn accept_authorized_input(&mut self, input: &Input) -> bool {
        let Some(frame) = self.displayed.as_ref() else { return false };
        if !self.connected
            || !self.capable
            || !self.input_consumer_ready
            || input.session != self.session
            || input.target != self.target
            || input.frame_sequence != frame.sequence
            || input.target_generation != self.target_generation
            || input.epoch != self.epoch
            || input.controller != self.controller
            || self.last_input.checked_add(1) != Some(input.sequence)
            || self.inputs.len() >= 64
        {
            return false;
        }
        let Some((x, y)) =
            map_point(frame.image, frame.viewport, input.display_width, input.display_height, input.x, input.y)
        else {
            return false;
        };
        self.last_input = input.sequence;
        self.inputs.push_back(AcceptedInput {
            sequence: input.sequence,
            epoch: self.epoch,
            target_generation: self.target_generation,
            #[cfg(test)]
            recibo: uuid::Uuid::new_v4(),
            x,
            y,
        });
        true
    }

    /// Retornar a sequência após executar a ação permite confirmar somente ações executadas.
    pub fn next_input(&mut self, authority: &impl Authority, recipient: &str) -> Option<AcceptedInput> {
        if !self.connected || !authority.is_owner(&self.session, recipient) {
            self.inputs.clear();
            return None;
        }
        self.inputs.pop_front()
    }

    fn input_still_current(&self, input: &AcceptedInput) -> bool {
        self.connected
            && self.capable
            && self.input_consumer_ready
            && self.epoch == input.epoch
            && self.target_generation == input.target_generation
    }

    pub fn rates(&self) -> (u8, u8) {
        (self.quality, self.fps)
    }

    pub fn acknowledge_frame(&mut self) {
        self.quality = self.quality.saturating_add(1).min(70);
        self.fps = self.fps.saturating_add(1).min(15);
    }
}

fn map_point(image: Size, viewport: Size, width: f64, height: f64, x: f64, y: f64) -> Option<(f64, f64)> {
    if ![width, height, x, y].iter().all(|v| v.is_finite()) || width <= 0.0 || height <= 0.0 {
        return None;
    }
    let scale = (width / f64::from(image.width)).min(height / f64::from(image.height));
    let shown_width = f64::from(image.width) * scale;
    let shown_height = f64::from(image.height) * scale;
    let left = (width - shown_width) / 2.0;
    let top = (height - shown_height) / 2.0;
    if x < left || y < top || x >= left + shown_width || y >= top + shown_height {
        return None;
    }
    Some(((x - left) / shown_width * f64::from(viewport.width), (y - top) / shown_height * f64::from(viewport.height)))
}

#[cfg(test)]
mod tests;
