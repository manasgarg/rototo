function register(lint)
  lint:rule({
    id = "policy/advisory",
    title = "Workspace policy advisory",
    help = "Review the advisory before release.",
    severity = "warning",
    target = "/variables/message",
    handler = "check_variable",
  })
end

function check_variable(workspace, variable)
  return {
    {
      message = "message variable has an advisory",
      path = "/declaration/value",
    },
  }
end
