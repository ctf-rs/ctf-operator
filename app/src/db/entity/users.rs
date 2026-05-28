use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    #[sea_orm(unique)]
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub discord_id: Option<String>,
    pub discord_username: Option<String>,
    pub team_id: Option<String>,
    pub team_name: Option<String>,
    #[sea_orm(default_expr = "Expr::current_timestamp()")]
    pub created_at: DateTimeWithTimeZone,

    // Relations
    #[sea_orm(has_many)]
    pub sessions: HasMany<super::sessions::Entity>,
    #[sea_orm(has_many)]
    pub flag_submissions: HasMany<super::flag_submissions::Entity>,
    #[sea_orm(has_many)]
    pub solved_challenges: HasMany<super::solved_challenges::Entity>,
    #[sea_orm(has_many)]
    pub instance_actions: HasMany<super::instance_actions::Entity>,
    #[sea_orm(has_many)]
    pub downloads: HasMany<super::downloads::Entity>,
    #[sea_orm(has_many)]
    pub instance_flags: HasMany<super::instance_flags::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
