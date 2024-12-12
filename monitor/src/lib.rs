use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use actix_cloud::{
    actix_web::web::Data,
    i18n::{i18n, Locale},
    memorydb,
    router::CSRFType,
    state::{GlobalState, ServerHandle},
    tokio::{runtime::Runtime, spawn},
};
use dashmap::DashMap;
use ecies::utils::generate_keypair;
use migration::migrator::Migrator;
use sea_orm_migration::MigratorTrait;
use server::Server;
use skynet_api::{
    ffi_rpc::{
        self,
        abi_stable::prefix_type::PrefixTypeTrait,
        async_ffi, async_trait, bincode,
        ffi_rpc_macro::{
            plugin_impl_call, plugin_impl_instance, plugin_impl_root, plugin_impl_trait,
        },
        registry::Registry,
    },
    permission::{IDTypes::PermManagePluginID, PermChecker, PERM_ALL, PERM_READ, PERM_WRITE},
    plugin::{PluginStatus, Request, Response},
    request::{Method, Router, RouterType},
    route,
    sea_orm::{DatabaseConnection, TransactionTrait},
    service::{SResult, Service, SKYNET_SERVICE},
    tracing::{error, info, warn},
    uuid,
    viewer::permissions::PermissionViewer,
    HyUuid, MenuItem, Skynet,
};
use skynet_api_agent::semver::VersionReq;
use skynet_api_monitor::{Agent, ID};
use ws::ShellService;

mod api;
mod migration;
mod server;
mod service;
mod ws;

include!(concat!(env!("OUT_DIR"), "/response.rs"));

#[plugin_impl_instance(|| Plugin {
    server: Server::new(),
    agent_api: Default::default(),
    shell: Default::default(),
    shell_binding: Default::default(),
    agent: Default::default(),
    view_id: Default::default(),
    db: Default::default(),
    state: Default::default(),
    runtime: Runtime::new().unwrap(),
})]
#[plugin_impl_root]
#[plugin_impl_call(skynet_api::plugin::api::PluginApi, skynet_api_monitor::Service)]
struct Plugin {
    server: Server,
    agent_api: OnceLock<skynet_api_agent::AgentService>,
    shell: DashMap<HyUuid, ShellService>,
    shell_binding: DashMap<HyUuid, HyUuid>,
    agent: DashMap<HyUuid, Agent>,
    view_id: OnceLock<HyUuid>,
    db: OnceLock<DatabaseConnection>,
    state: OnceLock<Data<GlobalState>>,
    runtime: Runtime,
}

#[plugin_impl_trait]
impl skynet_api::plugin::api::PluginApi for Plugin {
    async fn on_load(
        &self,
        reg: &Registry,
        mut skynet: Skynet,
        _runtime_path: PathBuf,
    ) -> SResult<Skynet> {
        self.runtime.block_on(async {
            let server: Service = reg.get(SKYNET_SERVICE).unwrap().into();
            skynet.logger.plugin_start(server);
            if let Some(api) = reg.get(&skynet_api_agent::ID.to_string()) {
                let api: skynet_api_agent::AgentService = api.into();
                if VersionReq::parse("^0.5.0")
                    .unwrap()
                    .matches(&api.api_version(reg).await)
                {
                    let _ = self.agent_api.set(api);
                }
            } else {
                warn!(plugin = %ID, "Agent plugin not enabled, auto update disabled");
            }

            let db = skynet.get_db().await?;
            Migrator::up(&db, None).await?;
            let _ = self.db.set(db);

            let tx = self.db.get().unwrap().begin().await?;
            let addr = if let Some(x) = Plugin::get_setting_address(&tx).await? {
                x
            } else {
                info!("Addr not found, using default");
                let ret = "0.0.0.0:4242";
                Plugin::set_setting_address(&tx, ret).await?;
                ret.to_string()
            };
            if Plugin::get_setting_shell(&tx).await?.is_none() {
                info!("Shell program not found, using default");
                Plugin::set_setting_shell(
                    &tx,
                    &[
                        String::from("/bin/bash"),
                        String::from("/bin/sh"),
                        String::from("C:\\Windows\\System32\\cmd.exe"),
                    ],
                )
                .await?;
            }
            let key = if let Some(x) = Plugin::get_setting_certificate(&tx).await? {
                x
            } else {
                info!("Cert not found, generating new one");
                let key = generate_keypair();
                Plugin::set_setting_certificate(&tx, &key.0).await?;
                key.0
            };
            let _ = self.view_id.set(
                PermissionViewer::find_or_init(
                    &tx,
                    &format!("view.plugin.{ID}"),
                    "plugin monitor viewer",
                )
                .await?
                .id,
            );
            self.init_agent(&tx).await?;
            tx.commit().await?;

            let _ = skynet.insert_menu(
                MenuItem {
                    id: HyUuid(uuid!("f47a0d3a-f09e-4e5d-b62c-0012225e5155")),
                    plugin: Some(ID),
                    name: String::from("menu.monitor"),
                    path: format!("/plugin/{ID}/config"),
                    checker: PermChecker::new_entry(
                        skynet.default_id[PermManagePluginID],
                        PERM_READ,
                    ),
                    ..Default::default()
                },
                1,
                Some(HyUuid(uuid!("cca5b3b0-40a3-465c-8b08-91f3e8d3b14d"))),
            );
            let _ = skynet.insert_menu(
                MenuItem {
                    id: HyUuid(uuid!("d2231000-53be-46ac-87ae-73fb3f76f18f")),
                    plugin: Some(ID),
                    name: String::from("menu.monitor"),
                    path: format!("/plugin/{ID}/view"),
                    checker: PermChecker::new_entry(*self.view_id.get().unwrap(), PERM_READ),
                    ..Default::default()
                },
                0,
                Some(HyUuid(uuid!("d00d36d0-6068-4447-ab04-f82ce893c04e"))),
            );

            spawn(async move {
                PLUGIN_INSTANCE
                    .server
                    .start(&addr, key)
                    .await
                    .map_err(|e| error!(address=addr, error=%e, "Failed to start server"))
            });

            let locale = Locale::new(skynet.config.lang.clone()).add_locale(i18n!("locales"));
            let state = GlobalState {
                memorydb: Arc::new(memorydb::default::DefaultBackend::new()),
                config: Default::default(),
                logger: None,
                locale,
                server: ServerHandle::default(),
            }
            .build();
            let _ = self.state.set(state);
            Ok(skynet)
        })
    }

    async fn on_register(&self, _: &Registry, skynet: Skynet, mut r: Vec<Router>) -> Vec<Router> {
        r.extend(vec![
            Router {
                path: format!("/plugins/{ID}/ws"),
                method: Method::Get,
                route: RouterType::Websocket(ID, String::from("ws::service")),
                checker: PermChecker::new_entry(*self.view_id.get().unwrap(), PERM_ALL),
                csrf: CSRFType::ForceParam,
            },
            Router {
                path: format!("/plugins/{ID}/passive_agents"),
                method: Method::Get,
                route: RouterType::Http(ID, String::from("api::get_passive_agents")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_READ),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/passive_agents"),
                method: Method::Post,
                route: RouterType::Http(ID, String::from("api::add_passive_agents")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_WRITE),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/passive_agents"),
                method: Method::Delete,
                route: RouterType::Http(ID, String::from("api::delete_passive_agents_batch")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_WRITE),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/passive_agents/{{paid}}"),
                method: Method::Put,
                route: RouterType::Http(ID, String::from("api::put_passive_agents")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_WRITE),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/passive_agents/{{paid}}"),
                method: Method::Delete,
                route: RouterType::Http(ID, String::from("api::delete_passive_agents")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_WRITE),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/passive_agents/{{paid}}/activate"),
                method: Method::Post,
                route: RouterType::Http(ID, String::from("api::activate_passive_agents")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_WRITE),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/agents"),
                method: Method::Get,
                route: RouterType::Http(ID, String::from("api::get_agents")),
                checker: PermChecker::new_script(&format!(
                    r#"new_entry("{}", PERM_READ).check(PERMISSION) || new_entry("{}", PERM_READ).check(PERMISSION)"#,
                    self.view_id.get().unwrap(),
                    skynet.default_id[PermManagePluginID]
                )),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/agents"),
                method: Method::Delete,
                route: RouterType::Http(ID, String::from("api::delete_agents")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_WRITE),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/agents/{{aid}}"),
                method: Method::Put,
                route: RouterType::Http(ID, String::from("api::put_agent")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_WRITE),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/agents/{{aid}}"),
                method: Method::Delete,
                route: RouterType::Http(ID, String::from("api::delete_agent")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_WRITE),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/agents/{{aid}}/reconnect"),
                method: Method::Post,
                route: RouterType::Http(ID, String::from("api::reconnect_agent")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_WRITE),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/settings"),
                method: Method::Get,
                route: RouterType::Http(ID, String::from("api::get_settings")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_READ),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/settings"),
                method: Method::Put,
                route: RouterType::Http(ID, String::from("api::put_settings")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_WRITE),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/settings/shell"),
                method: Method::Get,
                route: RouterType::Http(ID, String::from("api::get_settings_shell")),
                checker: PermChecker::new_entry(*self.view_id.get().unwrap(), PERM_READ),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/settings/certificate"),
                method: Method::Get,
                route: RouterType::Http(ID, String::from("api::get_settings_certificate")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_READ),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/settings/certificate"),
                method: Method::Post,
                route: RouterType::Http(ID, String::from("api::new_settings_certificate")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_WRITE),
                csrf: CSRFType::Header,
            },
            Router {
                path: format!("/plugins/{ID}/settings/server"),
                method: Method::Post,
                route: RouterType::Http(ID, String::from("api::post_server")),
                checker: PermChecker::new_entry(skynet.default_id[PermManagePluginID], PERM_WRITE),
                csrf: CSRFType::Header,
            },
        ]);
        r
    }

    async fn on_route(&self, reg: &Registry, name: String, req: Request) -> SResult<Response> {
        self.runtime.block_on(async {
            route!(reg, self.state.get().unwrap().clone(), name, req,
                "ws::service" => ws::service,
                "api::get_passive_agents" => api::get_passive_agents,
                "api::add_passive_agents" => api::add_passive_agents,
                "api::delete_passive_agents_batch" => api::delete_passive_agents_batch,
                "api::put_passive_agents" => api::put_passive_agents,
                "api::delete_passive_agents" => api::delete_passive_agents,
                "api::activate_passive_agents" => api::activate_passive_agents,
                "api::get_agents" => api::get_agents,
                "api::delete_agents" => api::delete_agents,
                "api::put_agent" => api::put_agent,
                "api::delete_agent" => api::delete_agent,
                "api::reconnect_agent" => api::reconnect_agent,
                "api::get_settings" => api::get_settings,
                "api::put_settings" => api::put_settings,
                "api::get_settings_shell" => api::get_settings_shell,
                "api::get_settings_certificate" => api::get_settings_certificate,
                "api::new_settings_certificate" => api::new_settings_certificate,
                "api::post_server" => api::post_server,
            )
        })
    }

    async fn on_translate(&self, _: &Registry, str: String, lang: String) -> String {
        self.state.get().unwrap().locale.translate(lang, str)
    }

    async fn on_unload(&self, _: &Registry, _status: PluginStatus) {
        self.server.stop();
        self.shell.clear();
        self.agent.clear();
    }
}
