export const getGitImportCustomName = (
  selectedCount: number,
  customName: string,
) => {
  const trimmedName = customName.trim()
  return selectedCount === 1 ? trimmedName || undefined : undefined
}
