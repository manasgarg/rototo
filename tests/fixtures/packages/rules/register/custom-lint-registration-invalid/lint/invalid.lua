function register(lint)
  lint:rule({
    id = "payments/check",
    title = "Payments check",
    help = "Fix the payments policy.",
    target = "package",
    handler = "check",
  })
end

function check(package, target)
  return {}
end
