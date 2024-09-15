use super::super::specifications::Specification;
use super::unit_of_work::UnitOfWork;

pub trait Repository<CreateDTOType, DBDTOType, SpecificationType: Specification>:
    Send + Sync + 'static
{
    async fn create_from_dto(user_create_data: &CreateDTOType, uow: &mut UnitOfWork) -> DBDTOType;

    async fn get_one_by(
        specification: SpecificationType,
        uow: &mut UnitOfWork,
    ) -> Option<DBDTOType>;
}
