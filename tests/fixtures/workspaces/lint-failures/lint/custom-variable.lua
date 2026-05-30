function lint(variable)
  return {
    {
      message = "custom lint rejected " .. variable.id,
      help = "Change the fixture or the Lua lint rule."
    }
  }
end
