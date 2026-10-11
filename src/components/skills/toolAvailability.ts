type ToolAvailability = {
  key: string
  enabled: boolean
  installed: boolean
}

export const isGlobalToolAvailable = (tool: ToolAvailability) =>
  tool.enabled && tool.installed

export const isProjectToolAvailable = (
  tool: ToolAvailability,
  projectEnabledToolKeys: string[],
) =>
  tool.enabled &&
  tool.installed &&
  projectEnabledToolKeys.includes(tool.key)

export const getAvailableProjectToolKeys = (
  tools: ToolAvailability[],
  projectEnabledToolKeys: string[],
) =>
  projectEnabledToolKeys.filter((toolKey) =>
    tools.some(
      (tool) =>
        tool.key === toolKey &&
        isProjectToolAvailable(tool, projectEnabledToolKeys),
    ),
  )
