use azalea::events::SentSpawnEvent;
use azalea::protocol::packets::ProtocolPacket;
use std::sync::Arc;
use std::time::Duration;
use tracing::Level;
use tracing::span;

use azalea::inventory::components::CustomName;
use azalea::protocol::common::client_information::HumanoidArm;
use azalea::protocol::packets::game::ClientboundGamePacket;
use azalea::{Account, Event, prelude::Component, swarm::SwarmBuilder};
use azalea::{ClientInformation, ecs::prelude::*, prelude::*, swarm::prelude::*};
use eyre::{Ok, bail};
use serde::Deserialize;
use tracing::info;
#[tokio::main]
async fn main() -> eyre::Result<()> {
	let destination: Arc<str> = std::env::var("INTROVERT_ISLAND")
		.expect("missing INTROVERT_ISLAND target username")
		.into();
	let mut accounts = vec![];
	for (key, value) in std::env::vars() {
		if key.starts_with("INTROVERT_ACCOUNT") {
			info!("Logging into account {value}");
			accounts.push(Account::microsoft(&value).await?);
			info!("Login successful");
		}
	}
	info!("Found {} accounts", accounts.len());
	for acc in &accounts {
		info!(" - {} {:?}", acc.username, acc.uuid);
	}
	if accounts.is_empty() {
		bail!("No accounts found. Please declare one using INTROVERT_ACCOUNT_{{N}}");
	}
	if !accounts.iter().any(|it| it.username == *destination) {
		tracing::warn!(
			"destination user name {} not found in accounts list",
			destination
		);
	}

	let mut builder = SwarmBuilder::new()
		.set_handler(handle)
		.set_swarm_handler(swarm_handle);

	for acc in accounts {
		let should_visit = if acc.username.eq_ignore_ascii_case(&destination) {
			None
		} else {
			Some(destination.clone())
		};
		builder = builder.add_account_with_state(
			acc,
			State {
				should_visit,
				last_screen_open: Default::default(),
				last_world_spawn: Default::default(),
			},
		);
	}

	builder
		.join_delay(Duration::from_secs(5))
		.set_swarm_state(SwarmState {})
		.start("mc.hypixel.net")
		.await?;
}

#[derive(Default, Clone, Component)]
struct State {
	last_world_spawn: Arc<tokio::sync::Mutex<Option<i32>>>,
	last_screen_open: Arc<tokio::sync::Mutex<Option<i32>>>,
	should_visit: Option<Arc<str>>,
}

#[derive(Default, Clone, Component, Resource)]
struct SwarmState {}

#[derive(Debug, Clone, Deserialize)]
#[allow(unused)]
struct Locraw {
	mode: Option<String>,
	server: Option<String>,
	gametype: Option<String>,
}

async fn handle(bot: Client, event: Event, state: State) -> eyre::Result<()> {
	let _span = span!(Level::INFO, "handle", account = bot.profile().name);
	let _enter = _span.enter();
	match event {
		Event::Init => {
			bot.set_client_information(ClientInformation {
				main_hand: HumanoidArm::Left, // this is very important
				..Default::default()
			})
			.await;
		}
		Event::Spawn => {
			tracing::info!("spawned, resetting /locraw timer");
			*state.last_world_spawn.lock().await = Some(0);
			*state.last_screen_open.lock().await = None;
		}
		Event::Chat(chat_packet) => {
			let chat_line = chat_packet.content();
			if chat_line.contains("Mana") || chat_line.contains("parkour") {
				return Ok(());
			}
			tracing::info!("Received chat: {}", chat_line);
			if chat_line.starts_with("{") {
				match serde_json::from_str::<'_, Locraw>(&chat_line) {
					Result::Ok(locraw) => {
						tracing::info!("Parsed locraw as {:?}", locraw);
						if locraw.server.as_deref() == Some("limbo") {
							bot.send_command_packet("lobby");
							*state.last_world_spawn.lock().await = Some(-300);
							tracing::warn!("running /lobby to escape limbo");
						} else if locraw.gametype.as_deref() != Some("SKYBLOCK") {
							bot.send_command_packet("skyblock");
							*state.last_world_spawn.lock().await = Some(-300);
							tracing::info!("running /skyblock to join skyblock");
						} else {
							match state.should_visit {
								Some(master) => {
									tracing::info!("running /visit to visit {}", master);
									bot.send_command_packet(&format!("visit {}", master))
								}
								None => {
									bot.send_command_packet("warp island");
									*state.last_world_spawn.lock().await = Some(-300);
									tracing::info!("running /warp island to warp to island");
								}
							}
						}
					}
					Err(err) => tracing::error!("Could not parse locraw {:?}", err),
				}
			}
		}
		Event::Tick => {
			{
				let mut sp = state.last_world_spawn.lock().await;
				if let Some(last) = *sp {
					*sp = Some(last + 1);
					if last == 200 {
						tracing::info!("Running /locraw");
						bot.send_command_packet("locraw");
					}
				}
			}
			{
				let mut sp = state.last_screen_open.lock().await;
				if let Some(last) = *sp {
					*sp = Some(last + 1);
					if last == 100 {
						let inv = bot.get_inventory();
						if let Some(content) = inv.contents() {
							let click_idx = content.iter().position(|it| {
								if let Some(data) = it.as_present() {
									let custom_name = data.get_component::<CustomName>();
									custom_name
										.filter(|it| {
											it.name
												.to_custom_format(
													|_style1, _style2| ("".into(), "".into()),
													|s| s.into(),
													|_style| "".into(),
													&Default::default(),
												)
												.contains("Visit player island")
										})
										.is_some()
								} else {
									false
								}
							});
							if let Some(pos) = click_idx {
								inv.left_click(pos);
								*state.last_world_spawn.lock().await = Some(-300);
							} else {
								tracing::error!(
									"Could not find visit itemstack in inventory {:?}",
									content
								)
							}
						} else {
							tracing::error!(
								"Could not find visit itemstack in missing inventory {:?}",
								inv
							)
						}
					}
				}
			}
		}
		Event::Packet(packet) => match &*packet {
			ClientboundGamePacket::OpenScreen(_) => {
				*state.last_screen_open.lock().await = Some(0);
			}
			ClientboundGamePacket::LevelParticles(_)
			| ClientboundGamePacket::Ping(_)
			| ClientboundGamePacket::BossEvent(_)
			| ClientboundGamePacket::EntityPositionSync(_)
			| ClientboundGamePacket::MoveEntityRot(_)
			| ClientboundGamePacket::MoveEntityPos(_)
			| ClientboundGamePacket::MoveEntityPosRot(_)
			| ClientboundGamePacket::RotateHead(_)
			| ClientboundGamePacket::ContainerSetSlot(_)
			| ClientboundGamePacket::SetObjective(_)
			| ClientboundGamePacket::SetEntityData(_)
			| ClientboundGamePacket::SetEntityMotion(_)
			| ClientboundGamePacket::PlayerInfoUpdate(_)
			| ClientboundGamePacket::PlayerInfoRemove(_)
			| ClientboundGamePacket::SetEquipment(_)
			| ClientboundGamePacket::Animate(_)
			| ClientboundGamePacket::AddEntity(_)
			| ClientboundGamePacket::LevelChunkWithLight(_)
			| ClientboundGamePacket::KeepAlive(_)
			| ClientboundGamePacket::SetPlayerTeam(_)
			| ClientboundGamePacket::RemoveEntities(_)
			| ClientboundGamePacket::UpdateAttributes(_) => {}
			_ => {
				// info!("packet : {}", packet.name())
			}
		},
		_ => {}
	}
	Ok(())
}

async fn swarm_handle(_swarm: Swarm, _event: SwarmEvent, _state: SwarmState) -> eyre::Result<()> {
	Ok(())
}
