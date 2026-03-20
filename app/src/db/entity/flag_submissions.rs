use sea_orm::entity::prelude::*;

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "flag_submissions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub user_id: i64,
    pub challenge_name: String,
    pub ctf_name: String,
    pub flag_name: String,
    pub submitted_flag: String,
    pub accepted: bool,
    #[sea_orm(default_value = false)]
    pub cheated: bool,
    pub cheated_instance_flag_id: Option<i64>,
    #[sea_orm(default_value = false)]
    pub admin_submission: bool,
    pub ip_address: Option<String>,
    #[sea_orm(default_expr = "Expr::current_timestamp()")]
    pub created_at: DateTimeWithTimeZone,

    // Relations
    #[sea_orm(belongs_to, from = "user_id", to = "id")]
    pub user: HasOne<super::users::Entity>,
    #[sea_orm(belongs_to, from = "cheated_instance_flag_id", to = "id")]
    pub cheated_instance_flag: Option<super::instance_flags::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
