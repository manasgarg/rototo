function register(lint)
  lint:rule({
    id = "fixture/custom-variable-rejected",
    title = "Custom variable lint rejected the variable",
    help = "Change the fixture or the Lua lint rule.",
    target = "variable=custom_lint",
    handler = "reject_variable",
  })
end

function reject_variable(package, variable)
  return {
    {
      message = "custom lint rejected " .. variable.id
    }
  }
end
