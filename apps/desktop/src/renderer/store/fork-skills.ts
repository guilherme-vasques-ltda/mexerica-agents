/** Wrapper público forte da frente de catálogo (D166 R7/R8). A UI usa só `passiveSkillsV2`:
 * `skillProtocol: 2` em todo pedido e envelope, encarnação do Hub e resultado incerto. Mantém
 * seleção/busy por Hub e usa SkillRequestScope para invalidar consultas após troca de Hub. */
export { passiveSkillsV2, SkillInstallationError, SkillMutationOutcomeUnknown, SkillRequestScope } from '../../shared/fork-skills'
export type {
  CatalogSkill, ExpectedSkillReceipt, InstallCatalogSkill, InstalledSkillV2, RemoveInstalledSkillV2, SkillAvailability,
  SkillClient, SkillContentStatus, SkillDetailsV2, SkillMutationV2, SkillReadiness, SkillReceipt,
} from '../../shared/fork-skills'
