function register(lint)
  lint:rule({
    id = "payments/check",
    title = "Payments check",
    help = "Fix the payments policy.",
    target = "workspace",
    handler = "check",
  })
end

function check(workspace, target)
  return {}
end
