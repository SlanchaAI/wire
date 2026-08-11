use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, anyhow};
use serde::Serialize;
use time::OffsetDateTime;

pub const TOPOLOGY_SCHEMA: &str = "wire-topology-v1";

#[derive(Clone, Debug, Serialize)]
pub struct TopologyReport {
    pub schema: &'static str,
    pub generated_at: String,
    pub machines: Vec<TopologyMachine>,
    pub sessions: Vec<TopologySession>,
    pub direct_links: Vec<DirectLink>,
    pub groups: Vec<TopologyGroup>,
    pub anomalies: Vec<TopologyAnomaly>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TopologyMachine {
    pub id: String,
    pub hostname: String,
    pub os: String,
    pub arch: String,
    pub identity_confidence: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TopologySession {
    pub machine_id: String,
    pub session: crate::operator::LiveSession,
}

#[derive(Clone, Debug, Serialize)]
pub struct DirectLink {
    pub id: String,
    pub source_did: String,
    pub target_did: String,
    pub state: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TopologyGroup {
    pub id: String,
    pub name: String,
    pub creator_did: String,
    pub epoch: u64,
    pub members: Vec<TopologyGroupMember>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TopologyGroupMember {
    pub did: String,
    pub tier: String,
    pub live: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct TopologyAnomaly {
    pub kind: String,
    pub subject_id: String,
    pub message: String,
}

#[derive(Clone)]
struct TopologySource {
    session: crate::operator::LiveSession,
    peers: Vec<crate::dash::PeerRow>,
    groups: Vec<crate::group::Group>,
}

enum GroupResolution {
    Accepted {
        group: crate::group::Group,
        holders: BTreeSet<String>,
    },
    Conflicted {
        epoch: u64,
        holders: BTreeSet<String>,
    },
}

pub fn collect_topology() -> Result<TopologyReport> {
    let sessions = crate::session::list_sessions()?;
    let live = crate::operator::collect_live_sessions_from(&sessions)?;
    let homes = sessions
        .iter()
        .map(|session| (session.name.as_str(), session.home_dir.as_path()))
        .collect::<BTreeMap<_, _>>();
    let mut sources = Vec::with_capacity(live.sessions.len());
    for session in live.sessions {
        let home = homes
            .get(session.id.as_str())
            .ok_or_else(|| anyhow!("live session was not present in the session inventory"))?;
        sources.push(TopologySource {
            peers: crate::dash::read_peers(home, Some(&session.did), Some(&session.handle)),
            groups: crate::group::list_groups_at(home)?,
            session,
        });
    }
    Ok(build_topology(sources, OffsetDateTime::now_utc()))
}

fn machine_id(session: &crate::operator::LiveSession) -> (String, String) {
    match &session.machine.fingerprint {
        Some(fingerprint) => (fingerprint.clone(), "verified".to_string()),
        None => (
            format!(
                "unverified:{}:{}:{}",
                session.machine.hostname, session.machine.os, session.machine.arch
            ),
            "unverified".to_string(),
        ),
    }
}

fn canonical_roster(group: &crate::group::Group) -> Vec<(String, String)> {
    let mut roster = group
        .members
        .iter()
        .map(|member| (member.did.clone(), member.tier.as_str().to_string()))
        .collect::<Vec<_>>();
    roster.sort();
    roster
}

fn groups_agree(left: &crate::group::Group, right: &crate::group::Group) -> bool {
    left.creator_did == right.creator_did && canonical_roster(left) == canonical_roster(right)
}

fn build_topology(
    mut sources: Vec<TopologySource>,
    generated_at: OffsetDateTime,
) -> TopologyReport {
    sources.sort_by(|left, right| left.session.did.cmp(&right.session.did));
    let live_dids = sources
        .iter()
        .map(|source| source.session.did.clone())
        .collect::<BTreeSet<_>>();

    let mut machines = BTreeMap::new();
    let mut topology_sessions = Vec::with_capacity(sources.len());
    for source in &sources {
        let (id, identity_confidence) = machine_id(&source.session);
        machines
            .entry(id.clone())
            .or_insert_with(|| TopologyMachine {
                id: id.clone(),
                hostname: source.session.machine.hostname.clone(),
                os: source.session.machine.os.clone(),
                arch: source.session.machine.arch.clone(),
                identity_confidence,
            });
        topology_sessions.push(TopologySession {
            machine_id: id,
            session: source.session.clone(),
        });
    }

    let mut observations = BTreeMap::<(String, String), BTreeSet<(String, String)>>::new();
    for source in &sources {
        for peer in &source.peers {
            if peer.introduced_via.is_some()
                || peer.did.is_empty()
                || !live_dids.contains(&peer.did)
                || peer.did == source.session.did
            {
                continue;
            }
            let (source_did, target_did) = if source.session.did < peer.did {
                (source.session.did.clone(), peer.did.clone())
            } else {
                (peer.did.clone(), source.session.did.clone())
            };
            observations
                .entry((source_did, target_did))
                .or_default()
                .insert((source.session.did.clone(), peer.did.clone()));
        }
    }
    let mut direct_links = Vec::with_capacity(observations.len());
    let mut anomalies = Vec::new();
    for ((source_did, target_did), directions) in observations {
        let id = format!("{source_did}:{target_did}");
        let bilateral = directions.contains(&(source_did.clone(), target_did.clone()))
            && directions.contains(&(target_did.clone(), source_did.clone()));
        let state = if bilateral { "bilateral" } else { "one-sided" };
        direct_links.push(DirectLink {
            id: id.clone(),
            source_did,
            target_did,
            state: state.to_string(),
        });
        if !bilateral {
            anomalies.push(TopologyAnomaly {
                kind: "one-sided-link".to_string(),
                subject_id: id,
                message: "Live sessions disagree about this direct link".to_string(),
            });
        }
    }

    let mut group_resolutions = BTreeMap::<String, GroupResolution>::new();
    for source in &sources {
        for group in &source.groups {
            let holder = source.session.did.clone();
            match group_resolutions.get_mut(&group.id) {
                None => {
                    group_resolutions.insert(
                        group.id.clone(),
                        GroupResolution::Accepted {
                            group: group.clone(),
                            holders: BTreeSet::from([holder]),
                        },
                    );
                }
                Some(GroupResolution::Accepted {
                    group: existing,
                    holders,
                }) if group.epoch > existing.epoch => {
                    *existing = group.clone();
                    holders.clear();
                    holders.insert(holder);
                }
                Some(GroupResolution::Accepted {
                    group: existing,
                    holders,
                }) if group.epoch == existing.epoch => {
                    if !groups_agree(existing, group) {
                        let epoch = existing.epoch;
                        let mut holders = std::mem::take(holders);
                        holders.insert(holder);
                        group_resolutions.insert(
                            group.id.clone(),
                            GroupResolution::Conflicted { epoch, holders },
                        );
                    } else {
                        holders.insert(holder);
                    }
                }
                Some(GroupResolution::Conflicted { epoch, .. }) if group.epoch > *epoch => {
                    group_resolutions.insert(
                        group.id.clone(),
                        GroupResolution::Accepted {
                            group: group.clone(),
                            holders: BTreeSet::from([holder]),
                        },
                    );
                }
                Some(GroupResolution::Conflicted { epoch, holders }) if group.epoch == *epoch => {
                    holders.insert(holder);
                }
                Some(GroupResolution::Accepted { .. })
                | Some(GroupResolution::Conflicted { .. }) => {}
            }
        }
    }
    let mut groups = Vec::new();
    for (id, resolution) in group_resolutions {
        match resolution {
            GroupResolution::Accepted { group, holders } => {
                let mut members = group
                    .members
                    .iter()
                    .map(|member| TopologyGroupMember {
                        did: member.did.clone(),
                        tier: member.tier.as_str().to_string(),
                        live: live_dids.contains(&member.did),
                    })
                    .collect::<Vec<_>>();
                for did in holders {
                    if members.iter().all(|member| member.did != did) {
                        members.push(TopologyGroupMember {
                            did,
                            tier: "introduced".to_string(),
                            live: true,
                        });
                    }
                }
                members.sort_by(|left, right| {
                    left.did
                        .cmp(&right.did)
                        .then_with(|| left.tier.cmp(&right.tier))
                });
                groups.push(TopologyGroup {
                    id: group.id,
                    name: group.name,
                    creator_did: group.creator_did,
                    epoch: group.epoch,
                    members,
                });
            }
            GroupResolution::Conflicted { .. } => anomalies.push(TopologyAnomaly {
                kind: "conflicting-group".to_string(),
                subject_id: id,
                message: "Live sessions disagree about the highest group roster".to_string(),
            }),
        }
    }
    anomalies.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.subject_id.cmp(&right.subject_id))
    });

    TopologyReport {
        schema: TOPOLOGY_SCHEMA,
        generated_at: generated_at
            .format(&time::format_description::well_known::Rfc3339)
            .expect("valid RFC3339 format description"),
        machines: machines.into_values().collect(),
        sessions: topology_sessions,
        direct_links,
        groups,
        anomalies,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::group::{Group, GroupTier, Member};
    use crate::operator::LiveSession;
    use crate::session_metadata::{
        HarnessDescriptor, IdentityDescriptor, MachineDescriptor, MetadataConfidence,
        ProjectDescriptor,
    };
    use time::OffsetDateTime;

    const ALICE: &str = "did:wire:alice-11111111";
    const BOB: &str = "did:wire:bob-22222222";
    const CAROL: &str = "did:wire:carol-33333333";

    fn generated_at() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap()
    }

    fn live(id: &str, did: &str, fingerprint: Option<&str>) -> LiveSession {
        LiveSession {
            id: id.to_string(),
            handle: id.to_string(),
            did: did.to_string(),
            emoji: "🦎".to_string(),
            primary_hex: "#45e456".to_string(),
            pid: 42,
            machine: MachineDescriptor {
                fingerprint: fingerprint.map(str::to_string),
                hostname: "wire-host".to_string(),
                os: "macos".to_string(),
                arch: "aarch64".to_string(),
                wire_version: "0.17.0".to_string(),
            },
            harness: HarnessDescriptor {
                kind: "codex-cli".to_string(),
                label: "Codex CLI".to_string(),
                mode: Some("interactive".to_string()),
                confidence: MetadataConfidence::Explicit,
                evidence: "test-fixture".to_string(),
            },
            identity: IdentityDescriptor {
                source: "codex-cli".to_string(),
                class: "session-keyed".to_string(),
                warning: None,
            },
            project: ProjectDescriptor::unknown(None),
            started_at: None,
            age_seconds: None,
            direct_link_count: 0,
            health: "healthy".to_string(),
        }
    }

    fn peer(did: &str) -> crate::dash::PeerRow {
        crate::dash::PeerRow {
            handle: did.to_string(),
            did: did.to_string(),
            tier: "VERIFIED".to_string(),
            introduced_via: None,
        }
    }

    fn group(id: &str, epoch: u64, creator_did: &str, members: &[(&str, GroupTier)]) -> Group {
        Group {
            id: id.to_string(),
            name: format!("group-{id}"),
            creator_did: creator_did.to_string(),
            epoch,
            members: members
                .iter()
                .map(|(did, tier)| Member {
                    handle: did.to_string(),
                    did: (*did).to_string(),
                    tier: *tier,
                    key_id: "key-id-secret".to_string(),
                    key: "key-secret".to_string(),
                })
                .collect(),
            relay_url: "https://relay.example".to_string(),
            slot_id: "slot-secret".to_string(),
            slot_token: "token-secret".to_string(),
            creator_sig: "signature-secret".to_string(),
        }
    }

    fn source(session: LiveSession) -> TopologySource {
        TopologySource {
            session,
            peers: Vec::new(),
            groups: Vec::new(),
        }
    }

    fn assert_no_forbidden_keys(value: &serde_json::Value, forbidden: &[&str]) {
        match value {
            serde_json::Value::Object(object) => {
                for (key, value) in object {
                    assert!(
                        !forbidden.contains(&key.as_str()),
                        "serialized topology leaked {key}"
                    );
                    assert_no_forbidden_keys(value, forbidden);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    assert_no_forbidden_keys(value, forbidden);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn same_fingerprint_sessions_share_one_verified_machine() {
        let report = build_topology(
            vec![
                source(live("alice", ALICE, Some("machine-1"))),
                source(live("bob", BOB, Some("machine-1"))),
            ],
            generated_at(),
        );

        assert_eq!(report.machines.len(), 1);
        assert_eq!(report.machines[0].id, "machine-1");
        assert_eq!(report.machines[0].identity_confidence, "verified");
        assert!(
            report
                .sessions
                .iter()
                .all(|session| session.machine_id == "machine-1")
        );
    }

    #[test]
    fn missing_fingerprint_uses_unverified_machine_id() {
        let report = build_topology(vec![source(live("alice", ALICE, None))], generated_at());

        assert_eq!(report.machines[0].id, "unverified:wire-host:macos:aarch64");
        assert_eq!(report.machines[0].identity_confidence, "unverified");
    }

    #[test]
    fn reciprocal_peers_produce_one_sorted_bilateral_edge() {
        let mut alice = source(live("alice", ALICE, Some("machine-1")));
        alice.peers.push(peer(BOB));
        let mut bob = source(live("bob", BOB, Some("machine-1")));
        bob.peers.push(peer(ALICE));

        let report = build_topology(vec![bob, alice], generated_at());

        assert_eq!(report.direct_links.len(), 1);
        assert_eq!(report.direct_links[0].id, format!("{ALICE}:{BOB}"));
        assert_eq!(report.direct_links[0].source_did, ALICE);
        assert_eq!(report.direct_links[0].target_did, BOB);
        assert_eq!(report.direct_links[0].state, "bilateral");
        assert!(report.anomalies.is_empty());
    }

    #[test]
    fn one_sided_peer_produces_edge_and_anomaly() {
        let mut alice = source(live("alice", ALICE, Some("machine-1")));
        alice.peers.push(peer(BOB));
        let report = build_topology(
            vec![alice, source(live("bob", BOB, Some("machine-1")))],
            generated_at(),
        );

        assert_eq!(report.direct_links.len(), 1);
        assert_eq!(report.direct_links[0].state, "one-sided");
        assert_eq!(report.anomalies.len(), 1);
        assert_eq!(report.anomalies[0].kind, "one-sided-link");
        assert_eq!(report.anomalies[0].subject_id, format!("{ALICE}:{BOB}"));
    }

    #[test]
    fn stale_peer_never_creates_a_node_or_edge() {
        let mut alice = source(live("alice", ALICE, Some("machine-1")));
        alice.peers.push(peer(CAROL));
        let report = build_topology(vec![alice], generated_at());

        assert_eq!(report.sessions.len(), 1);
        assert!(report.direct_links.is_empty());
    }

    #[test]
    fn group_membership_is_one_region_without_direct_links() {
        let mut alice = source(live("alice", ALICE, Some("machine-1")));
        alice.groups.push(group(
            "crew",
            1,
            ALICE,
            &[(ALICE, GroupTier::Creator), (BOB, GroupTier::Member)],
        ));
        let report = build_topology(
            vec![alice, source(live("bob", BOB, Some("machine-1")))],
            generated_at(),
        );

        assert_eq!(report.groups.len(), 1);
        assert!(report.direct_links.is_empty());
    }

    #[test]
    fn materialized_group_homes_appear_as_introduced_live_members() {
        let creator_roster = group("crew", 1, ALICE, &[(ALICE, GroupTier::Creator)]);
        let mut alice = source(live("alice", ALICE, Some("machine-1")));
        alice.groups.push(creator_roster.clone());
        let mut bob = source(live("bob", BOB, Some("machine-1")));
        bob.groups.push(creator_roster.clone());
        let mut carol = source(live("carol", CAROL, Some("machine-1")));
        carol.groups.push(creator_roster);

        let report = build_topology(vec![alice, bob, carol], generated_at());

        assert!(report.direct_links.is_empty());
        assert_eq!(report.groups.len(), 1);
        assert_eq!(
            report.groups[0]
                .members
                .iter()
                .map(|member| (member.did.clone(), member.tier.clone(), member.live))
                .collect::<Vec<_>>(),
            vec![
                (ALICE.to_string(), "creator".to_string(), true),
                (BOB.to_string(), "introduced".to_string(), true),
                (CAROL.to_string(), "introduced".to_string(), true),
            ]
        );
    }

    #[test]
    fn highest_group_epoch_wins() {
        let mut older_alice = source(live("alice", ALICE, Some("machine-1")));
        older_alice.groups.push(group(
            "crew",
            2,
            ALICE,
            &[(ALICE, GroupTier::Creator), (BOB, GroupTier::Member)],
        ));
        let mut older_bob = source(live("bob", BOB, Some("machine-1")));
        older_bob.groups.push(group(
            "crew",
            2,
            ALICE,
            &[(ALICE, GroupTier::Creator), (BOB, GroupTier::Member)],
        ));
        let mut newer_carol = source(live("carol", CAROL, Some("machine-1")));
        newer_carol.groups.push(group(
            "crew",
            3,
            ALICE,
            &[(ALICE, GroupTier::Creator), (CAROL, GroupTier::Member)],
        ));

        let report = build_topology(vec![older_alice, older_bob, newer_carol], generated_at());

        assert_eq!(report.groups.len(), 1);
        assert_eq!(report.groups[0].epoch, 3);
        assert_eq!(
            report.groups[0]
                .members
                .iter()
                .map(|member| (member.did.as_str(), member.tier.as_str()))
                .collect::<Vec<_>>(),
            vec![(ALICE, "creator"), (CAROL, "member")],
            "holders of lower-epoch copies must not return as introduced members"
        );
    }

    #[test]
    fn conflicting_equal_epoch_groups_are_suppressed_with_anomaly() {
        let mut alice = source(live("alice", ALICE, Some("machine-1")));
        alice.groups.push(group(
            "crew",
            2,
            ALICE,
            &[(ALICE, GroupTier::Creator), (BOB, GroupTier::Member)],
        ));
        let mut bob = source(live("bob", BOB, Some("machine-1")));
        bob.groups.push(group(
            "crew",
            2,
            BOB,
            &[(BOB, GroupTier::Creator), (ALICE, GroupTier::Member)],
        ));

        let report = build_topology(vec![alice, bob], generated_at());

        assert!(report.groups.is_empty());
        assert_eq!(report.anomalies.len(), 1);
        assert_eq!(report.anomalies[0].kind, "conflicting-group");
        assert_eq!(report.anomalies[0].subject_id, "crew");
    }

    #[test]
    fn historical_group_members_remain_sanitized_but_do_not_become_nodes() {
        let mut alice = source(live("alice", ALICE, Some("machine-1")));
        alice.groups.push(group(
            "crew",
            1,
            ALICE,
            &[(ALICE, GroupTier::Creator), (CAROL, GroupTier::Introduced)],
        ));
        let report = build_topology(vec![alice], generated_at());

        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.groups[0].members.len(), 2);
        assert!(!report.groups[0].members[1].live);
        assert_eq!(report.groups[0].members[1].did, CAROL);
    }

    #[test]
    fn serialized_topology_omits_secret_and_host_fields() {
        let mut alice = source(live("alice", ALICE, Some("machine-1")));
        alice
            .groups
            .push(group("crew", 1, ALICE, &[(ALICE, GroupTier::Creator)]));
        let serialized =
            serde_json::to_string(&build_topology(vec![alice], generated_at())).unwrap();
        let value = serde_json::from_str(&serialized).unwrap();
        assert_no_forbidden_keys(
            &value,
            &[
                "relay_url",
                "slot_id",
                "slot_token",
                "key_id",
                "key",
                "creator_sig",
                "home_dir",
                "command_line",
            ],
        );
    }
}
