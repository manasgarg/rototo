function register(lint)
  lint:rule({
    id = "policy/advisory",
    title = "Package policy advisory",
    help = "Review the advisory before release.",
    severity = "warning",
    target = "/variables/message",
    handler = "check_variable",
  })
end

function check_variable(package, variable)
  return {
    {
      message = "message variable has an advisory",
      path = "/declaration/value",
    },
  }
end
