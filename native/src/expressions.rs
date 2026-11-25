use crate::keyframes::{Lane, Property};
use crate::model::{Scene, Shape};
use crate::shape_style::VectorProperty;
use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::HashMap};

const MAX_SOURCE: usize = 1024;
const MAX_DEPTH: usize = 24;
const MAX_STEPS: usize = 4096;
const MAX_EVALUATION_DEPTH: usize = 96;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Expression {
    pub lane: Lane,
    pub source: String,
}

#[derive(Clone)]
enum Node {
    Number(f64),
    Word(String),
    String(String),
    Vector(Vec<Node>),
    Negative(Box<Node>),
    Binary(char, Box<Node>, Box<Node>),
    Call(String, Vec<Node>),
    Link {
        by_id: bool,
        layer: String,
        lane: Lane,
        channel: Option<usize>,
    },
}

struct Parser<'a> {
    rest: &'a str,
    steps: usize,
}

impl<'a> Parser<'a> {
    fn parse(source: &'a str) -> Result<Node, String> {
        if source.len() > MAX_SOURCE {
            return Err("The expression is too long.".into());
        }
        let mut parser = Self {
            rest: source,
            steps: 0,
        };
        let node = parser.sum(0)?;
        if !parser.rest.trim().is_empty() {
            return Err("Unexpected text after the expression.".into());
        }
        Ok(node)
    }

    fn eat(&mut self, c: char) -> bool {
        self.rest = self.rest.trim_start();
        if let Some(rest) = self.rest.strip_prefix(c) {
            self.rest = rest;
            true
        } else {
            false
        }
    }

    fn need(&mut self, c: char) -> Result<(), String> {
        if self.eat(c) {
            Ok(())
        } else {
            Err(format!("Expected '{c}'."))
        }
    }

    fn sum(&mut self, depth: usize) -> Result<Node, String> {
        let mut left = self.product(depth)?;
        loop {
            let op = if self.eat('+') {
                '+'
            } else if self.eat('-') {
                '-'
            } else {
                return Ok(left);
            };
            left = Node::Binary(op, Box::new(left), Box::new(self.product(depth)?));
        }
    }

    fn product(&mut self, depth: usize) -> Result<Node, String> {
        let mut left = self.atom(depth)?;
        loop {
            let op = if self.eat('*') {
                '*'
            } else if self.eat('/') {
                '/'
            } else if self.eat('%') {
                '%'
            } else {
                return Ok(left);
            };
            left = Node::Binary(op, Box::new(left), Box::new(self.atom(depth)?));
        }
    }

    fn atom(&mut self, depth: usize) -> Result<Node, String> {
        self.steps += 1;
        if depth > MAX_DEPTH || self.steps > 64 {
            return Err("The expression is too complex.".into());
        }
        let next = depth + 1;
        if self.eat('-') {
            return Ok(Node::Negative(Box::new(self.atom(next)?)));
        }
        if self.eat('+') {
            return self.atom(next);
        }
        if self.eat('(') {
            let node = self.sum(next)?;
            self.need(')')?;
            return Ok(node);
        }
        if self.eat('[') {
            return Ok(Node::Vector(self.list(']', next)?));
        }
        if self.rest.starts_with('"') {
            return self.string().map(Node::String);
        }
        if self
            .rest
            .starts_with(|c: char| c.is_ascii_digit() || c == '.')
        {
            return self.number().map(Node::Number);
        }
        let name = self
            .word()
            .ok_or("Expected a number, a value or a function.")?;
        if !self.eat('(') {
            return Ok(Node::Word(name.into()));
        }
        if name == "layer" || name == "layerId" {
            return self.link(name == "layerId");
        }
        Ok(Node::Call(name.into(), self.list(')', next)?))
    }

    fn list(&mut self, close: char, depth: usize) -> Result<Vec<Node>, String> {
        let mut items = Vec::new();
        if self.eat(close) {
            return Ok(items);
        }
        loop {
            items.push(self.sum(depth)?);
            if self.eat(close) {
                return Ok(items);
            }
            self.need(',')?;
        }
    }

    fn word(&mut self) -> Option<&'a str> {
        self.rest = self.rest.trim_start();
        if !self
            .rest
            .starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        {
            return None;
        }
        let end = self
            .rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(self.rest.len());
        let (word, rest) = self.rest.split_at(end);
        self.rest = rest;
        Some(word)
    }

    fn number(&mut self) -> Result<f64, String> {
        let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
        let mut end = digits(self.rest);
        if let Some(tail) = self.rest[end..].strip_prefix('.') {
            end += 1 + digits(tail);
        }
        if self.rest[end..].starts_with(['e', 'E']) {
            end += 1;
            if self.rest[end..].starts_with(['+', '-']) {
                end += 1;
            }
            end += digits(&self.rest[end..]);
        }
        let (number, rest) = self.rest.split_at(end);
        self.rest = rest;
        number
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or("That number is invalid.".into())
    }

    fn string(&mut self) -> Result<String, String> {
        self.rest = self.rest.trim_start();
        if !self.rest.starts_with('"') {
            return Err("Expected text in double quotes.".into());
        }
        let mut escaped = false;
        for (i, c) in self.rest.char_indices().skip(1) {
            if !escaped && c == '"' {
                let (quoted, rest) = self.rest.split_at(i + 1);
                self.rest = rest;
                return serde_json::from_str(quoted)
                    .map_err(|_| "Invalid text in double quotes.".into());
            }
            escaped = !escaped && c == '\\';
        }
        Err("Close the double quote.".into())
    }

    fn link(&mut self, by_id: bool) -> Result<Node, String> {
        let layer = self.string()?;
        self.need(')')?;
        self.need('.')?;
        let name = self.word().ok_or("Choose a property to link.")?;
        let lane = match name {
            "position" => Lane::Pose(Property::Position),
            "size" => Lane::Pose(Property::Size),
            "rotation" => Lane::Pose(Property::Rotation),
            "opacity" => Lane::Pose(Property::Opacity),
            "color" => Lane::Color,
            "depth" => Lane::Spatial(crate::spatial::Property::Depth),
            "rotationX" => Lane::Spatial(crate::spatial::Property::RotationX),
            "rotationY" => Lane::Spatial(crate::spatial::Property::RotationY),
            "orientation" => Lane::Spatial(crate::spatial::Property::Orientation),
            "pointOfInterest" => Lane::Spatial(crate::spatial::Property::Interest),
            "fieldOfView" => Lane::Spatial(crate::spatial::Property::FieldOfView),
            "focusDistance" => Lane::Spatial(crate::spatial::Property::FocusDistance),
            "aperture" => Lane::Spatial(crate::spatial::Property::Aperture),
            "property" => {
                self.need('(')?;
                let encoded = self.string()?;
                self.need(')')?;
                serde_json::from_str(&encoded).map_err(|_| "Unknown linked property.")?
            }
            _ => return Err(format!("Unknown linked property '{name}'.")),
        };
        let channel = if self.eat('.') {
            let channel = self.word().ok_or("Choose a component after the dot.")?;
            let index = match channel {
                "x" | "r" | "width" => 0,
                "y" | "g" | "height" => 1,
                "z" | "b" => 2,
                "component" => {
                    self.need('(')?;
                    self.rest = self.rest.trim_start();
                    let number = self.number()?;
                    self.need(')')?;
                    if !(1.0..=5.0).contains(&number) || number.fract() != 0.0 {
                        return Err("Choose a component from 1 through 5.".into());
                    }
                    number as usize - 1
                }
                _ => return Err(format!("Unknown component '{channel}'.")),
            };
            Some(index)
        } else {
            None
        };
        let (lane, channel) = if lane == Lane::Pose(Property::Position) && channel == Some(2) {
            (Lane::Spatial(crate::spatial::Property::Depth), None)
        } else {
            (lane, channel)
        };
        Ok(Node::Link {
            by_id,
            layer,
            lane,
            channel,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Numbers(Vec<f64>),
    Path(crate::paths::PathData),
}

impl Value {
    fn number(number: f64) -> Self {
        Self::Numbers(vec![number])
    }

    fn scalar(&self) -> Result<f64, String> {
        match self {
            Self::Numbers(values) if values.len() == 1 => Ok(values[0]),
            _ => Err("Expected a single number.".into()),
        }
    }

    fn map(self, f: impl Fn(f64) -> f64) -> Result<Self, String> {
        match self {
            Self::Numbers(values) => Self::checked(values.into_iter().map(f).collect()),
            _ => Err("This operation requires numbers.".into()),
        }
    }

    fn zip(self, other: Self, f: impl Fn(f64, f64) -> f64) -> Result<Self, String> {
        let (Self::Numbers(a), Self::Numbers(b)) = (self, other) else {
            return Err("This operation requires numbers.".into());
        };
        let n = a.len().max(b.len());
        if (a.len() != 1 && a.len() != n) || (b.len() != 1 && b.len() != n) {
            return Err("The vectors must have the same length.".into());
        }
        Self::checked(
            (0..n)
                .map(|i| f(a[i.min(a.len() - 1)], b[i.min(b.len() - 1)]))
                .collect(),
        )
    }

    fn checked(values: Vec<f64>) -> Result<Self, String> {
        if values.is_empty() || values.len() > 5 {
            return Err("A vector needs one to five numbers.".into());
        }
        if values
            .iter()
            .any(|v| !v.is_finite() || v.abs() > f32::MAX as f64)
        {
            return Err("The result must be a finite number. Check for division by zero.".into());
        }
        Ok(Self::Numbers(values))
    }
}

fn raw(shape: &Shape, lane: Lane, time: f64) -> Result<Value, String> {
    if lane == Lane::Vector(VectorProperty::Path) {
        let pose = shape.lane_pose(time);
        return shape
            .path_at(time, [pose.w, pose.h])
            .map(Value::Path)
            .ok_or("This layer has no path.".into());
    }
    Value::checked(
        (0..lane.channels().len())
            .map(|i| f64::from(lane.value(shape, time, i)))
            .collect(),
    )
}

struct Evaluator<'a> {
    scene: &'a Scene,
    time: f64,
    stack: Vec<(usize, Lane)>,
    memo: HashMap<(usize, Lane), Result<Value, String>>,
    steps: usize,
    depth: usize,
}

impl<'a> Evaluator<'a> {
    fn new(scene: &'a Scene, time: f64) -> Self {
        Self {
            scene,
            time,
            stack: Vec::new(),
            memo: HashMap::new(),
            steps: 0,
            depth: 0,
        }
    }

    fn value(&mut self, index: usize, lane: Lane) -> Result<Value, String> {
        let target = (index, lane);
        if let Some(value) = self.memo.get(&target) {
            return value.clone();
        }
        if self.stack.contains(&target) {
            return Err("These expressions link back to each other.".into());
        }
        if self.stack.len() >= MAX_DEPTH {
            return Err("There are too many layers in the link chain.".into());
        }
        if self.stack.is_empty() {
            self.steps = 0;
        }
        let shape = self
            .scene
            .shapes
            .get(index)
            .ok_or("The linked layer no longer exists.")?;
        if !lane.exists_on(shape) {
            return Err("The linked property no longer exists.".into());
        }
        let Some(source) = shape
            .expression(lane)
            .filter(|source| !source.trim().is_empty())
        else {
            return raw(shape, lane, self.time);
        };
        self.stack.push(target);
        let value = Parser::parse(source)
            .and_then(|node| self.eval(&node, index, lane))
            .and_then(|value| match &value {
                Value::Path(_) if lane == Lane::Vector(VectorProperty::Path) => Ok(value),
                Value::Numbers(values)
                    if lane != Lane::Vector(VectorProperty::Path)
                        && values.len() == lane.channels().len() =>
                {
                    Ok(value)
                }
                _ => Err(format!(
                    "{} needs {}.",
                    lane.label(),
                    if lane == Lane::Vector(VectorProperty::Path) {
                        "a path".into()
                    } else if lane.channels().len() == 1 {
                        "one number".into()
                    } else {
                        format!("a vector with {} numbers", lane.channels().len())
                    }
                )),
            })
            .and_then(|value| {
                // Linked properties read the same clamped values that their lanes show.
                let mut key = expression_key(shape, lane, self.time, value)?;
                Ok(match &mut key {
                    crate::keyframes::LaneKey::Vector(key)
                        if lane == Lane::Vector(VectorProperty::Path) =>
                    {
                        match &key.value {
                            crate::shape_style::VectorValue::Path(path) => {
                                Value::Path(path.clone())
                            }
                            _ => return Err("The expression did not return a path.".into()),
                        }
                    }
                    _ => Value::Numbers(
                        (0..lane.channels().len())
                            .map(|i| f64::from(key.value(i)))
                            .collect(),
                    ),
                })
            });
        self.stack.pop();
        self.memo.insert(target, value.clone());
        value
    }

    fn eval(&mut self, node: &Node, index: usize, lane: Lane) -> Result<Value, String> {
        self.steps += 1;
        if self.steps > MAX_STEPS {
            return Err("The expression took too many steps.".into());
        }
        if self.depth >= MAX_EVALUATION_DEPTH {
            return Err("This expression is nested too deeply.".into());
        }
        self.depth += 1;
        let value = self.eval_node(node, index, lane);
        self.depth -= 1;
        value
    }

    fn eval_node(&mut self, node: &Node, index: usize, lane: Lane) -> Result<Value, String> {
        let shape = &self.scene.shapes[index];
        match node {
            Node::Number(n) => Ok(Value::number(*n)),
            Node::String(_) => {
                Err("Text is only used to choose a loop mode or linked layer.".into())
            }
            Node::Word(name) => match name.as_str() {
                "value" => raw(shape, lane, self.time),
                "time" => Ok(Value::number(self.time)),
                "index" => Ok(Value::number(
                    self.scene.shapes[..=index]
                        .iter()
                        .filter(|shape| !shape.onion_ghost)
                        .count() as f64,
                )),
                _ => Err(format!("Unknown name '{name}'.")),
            },
            Node::Vector(nodes) => Value::checked(
                nodes
                    .iter()
                    .map(|n| self.eval(n, index, lane)?.scalar())
                    .collect::<Result<_, _>>()?,
            ),
            Node::Negative(node) => self.eval(node, index, lane)?.map(|n| -n),
            Node::Binary(op, left, right) => {
                let left = self.eval(left, index, lane)?;
                let right = self.eval(right, index, lane)?;
                left.zip(right, |a, b| match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    _ => a % b,
                })
            }
            Node::Call(name, nodes) => self.call(name, nodes, index, lane),
            Node::Link {
                by_id,
                layer,
                lane,
                channel,
            } => {
                let mut matches = self.scene.shapes.iter().enumerate().filter(|(_, shape)| {
                    if shape.onion_ghost {
                        false
                    } else if *by_id {
                        shape.id == *layer
                    } else {
                        shape.name == *layer
                    }
                });
                let (target, _) = matches
                    .next()
                    .ok_or_else(|| format!("Layer '{layer}' is missing."))?;
                if matches.next().is_some() {
                    return Err(format!(
                        "More than one layer is named '{layer}'. Use Link to… to choose one."
                    ));
                }
                let value = self.value(target, *lane)?;
                if let Some(channel) = channel {
                    return match value {
                        Value::Numbers(values) => values
                            .get(*channel)
                            .copied()
                            .map(Value::number)
                            .ok_or("That component does not exist.".into()),
                        _ => Err("A path has no numeric components.".into()),
                    };
                }
                Ok(value)
            }
        }
    }

    fn call(
        &mut self,
        name: &str,
        nodes: &[Node],
        index: usize,
        lane: Lane,
    ) -> Result<Value, String> {
        let shape = &self.scene.shapes[index];
        if matches!(name, "loopOut" | "loopIn") {
            let [Node::String(mode)] = nodes else {
                return Err(format!("{name} needs a loop mode in double quotes."));
            };
            return loop_keys(shape, lane, self.time, name == "loopIn", mode);
        }
        let args = nodes
            .iter()
            .map(|node| self.eval(node, index, lane))
            .collect::<Result<Vec<_>, _>>()?;
        match (name, args.as_slice()) {
            ("sin", [v]) => v.clone().map(f64::sin),
            ("cos", [v]) => v.clone().map(f64::cos),
            ("abs", [v]) => v.clone().map(f64::abs),
            ("round", [v]) => v.clone().map(f64::round),
            ("floor", [v]) => v.clone().map(f64::floor),
            ("min", [a, b]) => a.clone().zip(b.clone(), f64::min),
            ("max", [a, b]) => a.clone().zip(b.clone(), f64::max),
            ("clamp", [v, low, high]) => {
                let (low, high) = (low.scalar()?, high.scalar()?);
                if low > high {
                    return Err("Clamp's minimum must be at most its maximum.".into());
                }
                v.clone().map(|n| n.clamp(low, high))
            }
            ("lerp", [a, b, fraction]) => {
                let fraction = fraction.scalar()?;
                a.clone().zip(b.clone(), |a, b| a + (b - a) * fraction)
            }
            ("random", [seed]) => Ok(Value::number(noise(seed.scalar()?.to_bits()))),
            ("wiggle", [frequency, amount]) => {
                let spot = self.time * frequency.scalar()?;
                let amount = amount.scalar()?;
                let start = spot.floor();
                let fraction = spot - start;
                let fraction = fraction * fraction * (3.0 - 2.0 * fraction);
                let seed = shape.id.bytes().fold(0_u64, |hash, b| {
                    hash.wrapping_mul(131).wrapping_add(u64::from(b))
                });
                let sample = |at: f64, channel: usize| {
                    noise(
                        (seed ^ (at as i64 as u64))
                            .wrapping_add((channel as u64).wrapping_mul(0x9e3779b97f4a7c15)),
                    )
                };
                let Value::Numbers(values) = raw(shape, lane, self.time)? else {
                    return Err("Wiggle needs a numeric property.".into());
                };
                Value::checked(
                    values
                        .into_iter()
                        .enumerate()
                        .map(|(channel, v)| {
                            let a = sample(start, channel);
                            let b = sample(start + 1.0, channel);
                            v + amount * (2.0 * (a + (b - a) * fraction) - 1.0)
                        })
                        .collect(),
                )
            }
            _ => Err(format!(
                "Unknown function or wrong number of arguments for '{name}'."
            )),
        }
    }
}

fn noise(mut n: u64) -> f64 {
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((n ^ (n >> 31)) >> 11) as f64 / ((1_u64 << 53) as f64)
}

fn loop_keys(
    shape: &Shape,
    lane: Lane,
    time: f64,
    before: bool,
    mode: &str,
) -> Result<Value, String> {
    if !matches!(mode, "cycle" | "pingpong" | "offset") {
        return Err("Choose cycle, pingpong or offset.".into());
    }
    let keys = shape.lane_keys(lane);
    let Some(first) = keys.first().map(|key| key.time()) else {
        return raw(shape, lane, time);
    };
    let last = keys.last().map_or(first, |key| key.time());
    let span = last - first;
    if span <= 1e-8 || (before && time >= first) || (!before && time <= last) {
        return raw(shape, lane, time);
    }
    let cycle = ((time - first) / span).floor();
    let fraction = (time - first).rem_euclid(span);
    let at = if mode == "pingpong" && cycle.rem_euclid(2.0) >= 1.0 {
        last - fraction
    } else {
        first + fraction
    };
    let value = raw(shape, lane, at)?;
    if mode != "offset" {
        return Ok(value);
    }
    let delta = raw(shape, lane, last)?.zip(raw(shape, lane, first)?, |a, b| (a - b) * cycle)?;
    value.zip(delta, |a, b| a + b)
}

impl Shape {
    pub fn expression(&self, lane: Lane) -> Option<&str> {
        self.expressions
            .iter()
            .find(|expression| expression.lane == lane)
            .map(|expression| expression.source.as_str())
    }

    pub fn set_expression(&mut self, lane: Lane, source: Option<String>) {
        self.expressions
            .retain(|expression| expression.lane != lane);
        if let Some(source) = source {
            self.expressions.push(Expression { lane, source });
        }
    }
}

pub fn evaluate(scene: &Scene, index: usize, lane: Lane, time: f64) -> Result<Value, String> {
    Evaluator::new(scene, time).value(index, lane)
}

fn expression_key(
    shape: &Shape,
    lane: Lane,
    time: f64,
    value: Value,
) -> Result<crate::keyframes::LaneKey, String> {
    use crate::keyframes::LaneKey;
    use crate::shape_style::{VectorKey, VectorValue};
    let mut key = match value {
        Value::Path(path) => LaneKey::Vector(VectorKey::new(
            time,
            VectorProperty::Path,
            VectorValue::Path(path),
        )),
        Value::Numbers(values) => {
            let mut key = shape
                .numeric_lane_key(lane, time)
                .ok_or("This property cannot be keyed.")?;
            for (channel, v) in values.into_iter().enumerate() {
                key.set_value(channel, v as f32, false);
            }
            key
        }
    };
    key.set_easing("linear", None);
    Ok(key)
}

fn active(shape: &Shape) -> impl Iterator<Item = Lane> + '_ {
    shape
        .expressions
        .iter()
        .filter(|e| !e.source.trim().is_empty() && e.lane.exists_on(shape))
        .map(|e| e.lane)
}

fn apply_sample(evaluator: &mut Evaluator<'_>, index: usize, copy: &mut Shape) {
    let shape = &evaluator.scene.shapes[index];
    let time = evaluator.time;
    for lane in active(shape) {
        if let Ok(value) = evaluator.value(index, lane)
            && let Ok(key) = expression_key(shape, lane, time, value)
        {
            copy.put_lane_key(time, key);
        }
    }
}

pub(crate) fn sample_layer<'a>(scene: &Scene, shape: &'a Shape, time: f64) -> Cow<'a, Shape> {
    if active(shape).next().is_none() {
        return Cow::Borrowed(shape);
    }
    let index = scene.shapes.iter().position(|layer| layer.id == shape.id);
    let mut copy = shape.clone();
    if let Some(i) = index
        && std::ptr::eq(&scene.shapes[i], shape)
    {
        apply_sample(&mut Evaluator::new(scene, time), i, &mut copy);
    } else {
        // A clipboard template may differ from the current layer with the same id.
        let mut context = scene.clone();
        let i = index.unwrap_or(context.shapes.len());
        if let Some(layer) = context.shapes.get_mut(i) {
            *layer = shape.clone();
        } else {
            context.shapes.push(shape.clone());
        }
        apply_sample(&mut Evaluator::new(&context, time), i, &mut copy);
    }
    Cow::Owned(copy)
}

pub(crate) fn at_time(scene: &Scene, time: f64) -> Cow<'_, Scene> {
    if !scene
        .shapes
        .iter()
        .any(|shape| active(shape).next().is_some())
    {
        return Cow::Borrowed(scene);
    }
    let mut evaluator = Evaluator::new(scene, time);
    let mut sampled = scene.clone();
    for (i, copy) in sampled.shapes.iter_mut().enumerate() {
        apply_sample(&mut evaluator, i, copy);
    }
    Cow::Owned(sampled)
}

pub(crate) fn bake(scene: &Scene, fps: f64) -> Result<Cow<'_, Scene>, String> {
    if !scene
        .shapes
        .iter()
        .any(|shape| active(shape).next().is_some())
    {
        return Ok(Cow::Borrowed(scene));
    }
    let mut baked = scene.clone();
    let tracks: Vec<_> = scene
        .shapes
        .iter()
        .enumerate()
        .flat_map(|(i, shape)| active(shape).map(move |lane| (i, lane)))
        .collect();
    let frames = (scene.duration * fps).ceil() as usize;
    if tracks.len().saturating_mul(frames.saturating_add(1)) > 240_000 {
        return Err("This export has too many expression samples. Shorten the composition or remove expressions.".into());
    }
    for &(i, lane) in &tracks {
        for key in scene.shapes[i].lane_keys(lane) {
            baked.shapes[i].remove_lane_key(lane, key.time());
        }
        baked.shapes[i].set_expression(lane, None);
    }
    for frame in 0..=frames {
        let time = (frame as f64 / fps).min(scene.duration);
        let mut evaluator = Evaluator::new(scene, time);
        for &(i, lane) in &tracks {
            let shape = &scene.shapes[i];
            let result = evaluator
                .value(i, lane)
                .and_then(|value| expression_key(shape, lane, time, value));
            let key =
                result.map_err(|error| format!("{} · {}: {error}", shape.name, lane.label()))?;
            baked.shapes[i].put_lane_key(time, key);
        }
    }
    Ok(Cow::Owned(baked))
}

impl Shape {
    pub(crate) fn expression_lanes(&self) -> impl Iterator<Item = Lane> + '_ {
        Lane::ALL
            .into_iter()
            .filter(|lane| {
                !self.is_camera()
                    || matches!(lane, Lane::Pose(Property::Position | Property::Rotation))
            })
            .chain(self.spatial_lanes())
            .chain(
                self.vector
                    .iter()
                    .flat_map(|vector| vector.properties().into_iter().map(Lane::Vector)),
            )
            .chain(self.masks.iter().flat_map(|mask| {
                crate::compositing::MaskProperty::ALL
                    .into_iter()
                    .map(move |p| Lane::Mask(mask.id, p))
            }))
            .chain(self.effects.iter().flat_map(|effect| {
                effect
                    .kind
                    .controls()
                    .iter()
                    .map(move |control| Lane::Effect(effect.id, control.parameter))
            }))
            .chain(self.text_layout.iter().flat_map(|text| {
                text.style_lanes().chain(
                    text.animators
                        .iter()
                        .flat_map(|animator| text.animator_lanes(animator.id)),
                )
            }))
    }
}

pub(crate) fn link_source(shape: &Shape, lane: Lane, channel: Option<usize>) -> Option<String> {
    let id = serde_json::to_string(&shape.id).ok()?;
    let property = serde_json::to_string(&lane).ok()?;
    let quoted = serde_json::to_string(&property).ok()?;
    let component = channel.map_or(String::new(), |i| format!(".component({})", i + 1));
    Some(format!("layerId({id}).property({quoted}){component}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROTATION: Lane = Lane::Pose(Property::Rotation);
    const POSITION: Lane = Lane::Pose(Property::Position);

    fn setup() -> Scene {
        let mut scene = Scene::empty("expressions".into());
        scene.add_shape("card".into(), "rect");
        scene.shapes[0].name = "Card".into();
        scene.add_shape("arrow".into(), "arrow");
        scene.shapes[1].name = "Arrow".into();
        scene
    }

    fn run(scene: &mut Scene, source: &str, time: f64) -> Result<Value, String> {
        scene.shapes[0].set_expression(ROTATION, Some(source.into()));
        evaluate(scene, 0, ROTATION, time)
    }

    #[test]
    fn precedence_functions_vectors_and_plain_errors() {
        let mut scene = setup();
        assert_eq!(
            run(&mut scene, "2 + 3 * (8 - 2) % 5 - -1 + index + time", 0.5),
            Ok(Value::number(7.5))
        );
        assert_eq!(
            run(
                &mut scene,
                "round(lerp(min(2, 3), clamp(10, 0, 8), .75)) + abs(-2) + max(floor(1.9), 0) + sin(0) + cos(0)",
                0.0
            ),
            Ok(Value::number(11.0))
        );
        scene.shapes[0].set_expression(POSITION, Some("value + [5, -2] * 2".into()));
        let pose = scene.shapes[0].pose(0.0);
        assert_eq!(
            evaluate(&scene, 0, POSITION, 0.0),
            Ok(Value::Numbers(vec![
                (pose.x + 10.0).into(),
                (pose.y - 4.0).into()
            ]))
        );
        for (source, message) in [
            ("clamp(2, 7, 1)", "minimum"),
            ("1 / 0", "division by zero"),
            ("1 + (2 * 3", "Expected ')'"),
            ("[1,2]", "one number"),
            ("mystery", "Unknown name"),
        ] {
            assert!(run(&mut scene, source, 0.0).unwrap_err().contains(message));
        }
        assert!(
            run(&mut scene, &"(".repeat(60), 0.0)
                .unwrap_err()
                .contains("complex")
        );
    }

    #[test]
    fn wiggle_and_random_are_seeded_and_stable_when_scrubbing() {
        let mut scene = setup();
        let source = "wiggle(3, 12) + random(15)";
        let first = run(&mut scene, source, 0.56).unwrap();
        assert_ne!(run(&mut scene, source, 0.9).unwrap(), first);
        assert_eq!(run(&mut scene, source, 0.56).unwrap(), first);
        assert_ne!(
            run(&mut scene, "wiggle(3, 12) + random(16)", 0.56).unwrap(),
            first
        );
        scene.shapes[1].set_expression(ROTATION, Some(source.into()));
        assert_ne!(evaluate(&scene, 1, ROTATION, 0.56).unwrap(), first);
    }

    #[test]
    fn looping_uses_raw_keys_and_their_easing_on_both_ends() {
        let mut scene = setup();
        let shape = &mut scene.shapes[0];
        shape.ease = "linear".into();
        shape.remove_lane_key(ROTATION, 0.0);
        for (t, r) in [(1.0, 10.0), (2.0, 30.0)] {
            shape.key_property(
                t,
                Property::Rotation,
                crate::model::Pose { r, ..shape.pose(t) },
            );
        }
        for (mode, next, prev) in [
            ("cycle", 15.0, 25.0),
            ("pingpong", 25.0, 15.0),
            ("offset", 35.0, 5.0),
        ] {
            assert_eq!(
                run(&mut scene, &format!("loopOut(\"{mode}\")"), 2.25),
                Ok(Value::number(next))
            );
            assert_eq!(
                run(&mut scene, &format!("loopIn(\"{mode}\")"), 0.75),
                Ok(Value::number(prev))
            );
        }
        assert_eq!(
            run(&mut scene, "loopOut(\"cycle\")", 2.0),
            Ok(Value::number(30.0))
        );
    }

    #[test]
    fn links_resolve_vectors_components_names_and_ids_and_reject_cycles() {
        let mut scene = setup();
        scene.shapes[1].set_expression(POSITION, Some("[time * 10, 50]".into()));
        assert_eq!(
            run(&mut scene, r#"layer("Arrow").position.x + 5"#, 2.0),
            Ok(Value::number(25.0))
        );
        let stable = link_source(&scene.shapes[1], POSITION, Some(1)).unwrap();
        scene.shapes[1].name = "Renamed".into();
        assert_eq!(run(&mut scene, &stable, 2.0), Ok(Value::number(50.0)));
        scene.shapes[1].set_expression(POSITION, Some(r#"[layer("Card").rotation, 10]"#.into()));
        assert!(
            run(&mut scene, r#"layer("Renamed").position.x"#, 0.0)
                .unwrap_err()
                .contains("link back")
        );
        assert!(
            run(&mut scene, r#"layer("Missing").rotation"#, 0.0)
                .unwrap_err()
                .contains("missing")
        );
        assert_eq!(
            at_time(&scene, 0.0).shapes[0].pose(0.0),
            scene.shapes[0].pose(0.0)
        );
    }

    #[test]
    fn baked_keys_match_evaluated_values_at_every_export_frame() {
        let mut scene = setup();
        scene.duration = 1.23;
        scene.shapes[0]
            .set_expression(POSITION, Some("value + [time * 30, sin(time) * 60]".into()));
        scene.shapes[1].set_expression(ROTATION, Some(r#"layer("Card").position.x * 0.25"#.into()));
        let baked = bake(&scene, 30.0).unwrap();
        for step in 0..=(scene.duration * 30.0).ceil() as usize {
            let t = (step as f64 / 30.0).min(scene.duration);
            for (index, lane) in [(0, POSITION), (1, ROTATION)] {
                let Value::Numbers(values) = evaluate(&scene, index, lane, t).unwrap() else {
                    panic!("numeric lane");
                };
                let key = baked.shapes[index].lane_key(lane, t).unwrap();
                assert_eq!(key.easing("hold").0, "linear");
                for (channel, value) in values.into_iter().enumerate() {
                    assert_eq!(key.value(channel), value as f32);
                }
            }
        }
        assert!(
            baked
                .shapes
                .iter()
                .all(|shape| shape.expressions.is_empty())
        );
        assert!(
            scene
                .shapes
                .iter()
                .all(|shape| !shape.expressions.is_empty())
        );
    }
}
