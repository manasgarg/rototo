function register(lint)
  lint:rule({
    id = "fixture/custom-value-rejected",
    title = "Custom value lint rejected a value",
    help = "Change the fixture value or the Lua lint rule.",
    target = "variable=custom_value_lint",
    handler = "reject_value",
  })
end

function reject_value(package, variable)
  return {
    {
      message = "custom value lint rejected "
        .. variable.id .. ".default",
      path = "/resolve/default",
    }
  }
end
