function lint_value(value)
  return {
    {
      message = "custom value lint rejected " .. value.variable.id .. "." .. value.name,
      help = "Change the fixture value or the Lua lint rule."
    }
  }
end
